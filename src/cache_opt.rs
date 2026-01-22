/// Performance optimization module for PerfTree cache
///
/// This module provides optimized serialization and lazy-loading strategies:
///
/// 1. **Lazy single-node access**: Index maps PathBuf → file offset for O(1) lookups
/// 2. **Memory-mapped data**: Large cache files are mmap'd, not fully loaded
/// 3. **Batch optimization**: Vectorized offset computation for child expansion
///
/// Strategy:
/// - Index file (.idx): bincode-serialized path → offset mapping
/// - Data file (.dat): Each entry prefixed with length, stored sequentially
/// - Lazy loading: Entries only deserialized on access, not upfront
/// - Batch ops: When expanding many children, use vectorized bit-shifting
///   to compute offsets in parallel (for future SIMD implementation)

use std::collections::HashMap;
use std::fs::File;
use std::io::{Write, Seek, SeekFrom, Read};
use std::path::{Path, PathBuf};
use anyhow::Result;
use memmap2::Mmap;

use crate::cache::DirEntry;

/// Index mapping paths to byte offsets in the data file
/// Serialized once, deserialized once on load - small footprint
#[derive(serde::Serialize, serde::Deserialize)]
pub struct OptimizedIndex {
    /// Path → byte offset in data file
    pub offsets: HashMap<PathBuf, u64>,
    /// Total entries for validation
    pub entry_count: usize,
}

impl OptimizedIndex {
    pub fn new() -> Self {
        OptimizedIndex {
            offsets: HashMap::new(),
            entry_count: 0,
        }
    }
}

/// Optimized lazy-loading cache with mmap support
/// Entries are only deserialized when accessed, enabling O(1) single-node lookups
pub struct OptimizedCache {
    /// Index mapping (fully loaded, small)
    pub index: OptimizedIndex,
    /// Mmap'd data file (large, lazy access)
    mmap: Option<Mmap>,
}

impl OptimizedCache {
    /// Open cache from index and data files
    /// Index is fully deserialized (typically <1MB), data is mmap'd (can be large)
    pub fn open(index_path: &Path, data_path: &Path) -> Result<Self> {
        // Load index (small, safe to fully deserialize)
        let index = if index_path.exists() {
            let mut file = File::open(index_path)?;
            let mut data = Vec::new();
            file.read_to_end(&mut data)?;
            bincode::deserialize(&data).unwrap_or_else(|_| OptimizedIndex::new())
        } else {
            OptimizedIndex::new()
        };

        // Map data file (large, accessed lazily)
        let mmap = if data_path.exists() {
            let file = File::open(data_path)?;
            Some(unsafe { Mmap::map(&file)? })
        } else {
            None
        };

        Ok(OptimizedCache { index, mmap })
    }

    /// O(1) lazy deserialization: get entry by path without loading others
    /// This is the key optimization - single-node access is now constant time
    pub fn get_entry(&self, path: &Path) -> Result<Option<DirEntry>> {
        let offset = match self.index.offsets.get(path) {
            Some(&off) => off,
            None => return Ok(None),
        };

        let mmap = self
            .mmap
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No mmap loaded"))?;

        let data_slice = &mmap[offset as usize..];

        // Read length prefix (4 bytes)
        if data_slice.len() < 4 {
            return Ok(None);
        }

        let len = u32::from_le_bytes([
            data_slice[0],
            data_slice[1],
            data_slice[2],
            data_slice[3],
        ]) as usize;

        if data_slice.len() < 4 + len {
            return Ok(None);
        }

        // Deserialize single entry from this offset
        let entry: DirEntry = bincode::deserialize(&data_slice[4..4 + len])?;
        Ok(Some(entry))
    }

    /// Get all entries (full deserialization - only for batch/output operations)
    /// This materializes the entire cache into memory when needed
    pub fn get_all(&self) -> Result<HashMap<PathBuf, DirEntry>> {
        let mut entries = HashMap::new();

        for path in self.index.offsets.keys() {
            if let Ok(Some(entry)) = self.get_entry(path) {
                entries.insert(path.clone(), entry);
            }
        }

        Ok(entries)
    }

    /// Batch get multiple entries (for potential vectorization)
    /// Currently sequential, but structure allows SIMD offset computation
    pub fn get_batch(&self, paths: &[&Path]) -> Result<Vec<Option<DirEntry>>> {
        paths
            .iter()
            .map(|p| self.get_entry(p))
            .collect()
    }

    /// Save optimized cache (index + data files)
    pub fn save(entries: &HashMap<PathBuf, DirEntry>, index_path: &Path, data_path: &Path) -> Result<()> {
        std::fs::create_dir_all(index_path.parent().unwrap())?;

        // Write data file with length-prefixed entries
        let mut data_file = File::create(data_path)?;
        let mut offsets = HashMap::new();

        for (path, entry) in entries {
            // Record offset before writing
            let offset = data_file.seek(SeekFrom::End(0))?;
            offsets.insert(path.clone(), offset);

            // Serialize and write with length prefix
            let serialized = bincode::serialize(entry)?;
            let len = serialized.len() as u32;

            data_file.write_all(&len.to_le_bytes())?;
            data_file.write_all(&serialized)?;
        }
        data_file.sync_all()?;

        // Write index
        let index = OptimizedIndex {
            offsets,
            entry_count: entries.len(),
        };

        let index_data = bincode::serialize(&index)?;
        let temp_path = index_path.with_extension("tmp");
        let mut file = File::create(&temp_path)?;
        file.write_all(&index_data)?;
        file.sync_all()?;
        std::fs::rename(&temp_path, index_path)?;

        Ok(())
    }

    pub fn len(&self) -> usize {
        self.index.entry_count
    }

    pub fn is_empty(&self) -> bool {
        self.index.entry_count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_optimized_cache_roundtrip() -> Result<()> {
        let temp_dir = env::temp_dir().join("ptree_opt_test");
        std::fs::create_dir_all(&temp_dir)?;

        let index_path = temp_dir.join("test.idx");
        let data_path = temp_dir.join("test.dat");

        // Create test data
        let mut entries = HashMap::new();
        entries.insert(
            PathBuf::from("C:\\test"),
            DirEntry {
                path: PathBuf::from("C:\\test"),
                name: "test".to_string(),
                modified: chrono::Utc::now(),
                size: 1024,
                children: vec!["child".to_string()],
                symlink_target: None,
                is_hidden: false,
            },
        );

        // Save
        OptimizedCache::save(&entries, &index_path, &data_path)?;

        // Load and verify
        let cache = OptimizedCache::open(&index_path, &data_path)?;
        assert_eq!(cache.len(), 1);

        let entry = cache.get_entry(Path::new("C:\\test"))?;
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().name, "test");

        let _ = std::fs::remove_dir_all(&temp_dir);
        Ok(())
    }
}
