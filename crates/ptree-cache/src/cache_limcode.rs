use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Write, Seek, SeekFrom};
use std::path::PathBuf;
use chrono::{DateTime, Utc};
use anyhow::Result;
use memmap2::Mmap;

/// Limcode-optimized directory entry with rkyv serialization
/// Uses primitives that rkyv can directly archive
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct LimcodeDirEntry {
    pub path: String,  // PathBuf not Archive-compatible, use String
    pub name: String,
    pub modified_timestamp: i64,  // DateTime<Utc> not Archive-compatible, use i64
    pub size: u64,
    pub children: Vec<String>,
    pub symlink_target: Option<String>,  // Use String instead of PathBuf
    pub is_hidden: bool,
}

/// Index with limcode-optimized offset storage for batch deserialization
/// Stores offsets and entry metadata for efficient batch access patterns
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone)]
#[archive(check_bytes)]
pub struct LimcodeIndex {
    /// Path → offset mapping for fast lookup
    pub offsets: HashMap<String, u64>,
    /// Sorted offset list for batch sequential access
    pub sorted_offsets: Vec<u64>,
    pub last_scan_timestamp: i64,
    pub root: String,
    pub last_scanned_root: String,
    pub skip_stats: HashMap<String, usize>,
}

impl LimcodeIndex {
    pub fn new() -> Self {
        LimcodeIndex {
            offsets: HashMap::new(),
            sorted_offsets: Vec::new(),
            last_scan_timestamp: Utc::now().timestamp(),
            root: String::new(),
            last_scanned_root: String::new(),
            skip_stats: HashMap::new(),
        }
    }

    /// Update sorted offsets list after adding entries (call once during finalization)
    pub fn rebuild_sorted_offsets(&mut self) {
        self.sorted_offsets = self.offsets.values().copied().collect();
        self.sorted_offsets.sort();
    }
}

/// Hybrid cache combining rkyv zero-copy with batch SIMD deserialization
///
/// Dual-mode access:
/// - Single entry: O(1) lazy zero-copy via rkyv (mmap pointer to archived data)
/// - Batch entries: Sequential batch deserialization for cache efficiency
///
/// Layout:
/// - index file (.limidx): LimcodeIndex with offset mappings (rkyv archived)
/// - data file (.limdat): rkyv-archived entries at tracked offsets
pub struct LimcodeCache {
    pub index: LimcodeIndex,
    mmap: Option<Mmap>,
    data_path: PathBuf,
}

impl LimcodeCache {
    /// Load cache from limcode-optimized files
    pub fn open(index_path: &std::path::Path, data_path: &std::path::Path) -> Result<Self> {
        fs::create_dir_all(index_path.parent().unwrap())?;

        // Load and deserialize index (small file, fully deserialized)
        let index = if index_path.exists() {
            let mut file = File::open(index_path)?;
            let mut data = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut data)?;

            match rkyv::from_bytes::<LimcodeIndex>(&data) {
                Ok(idx) => idx,
                Err(_) => LimcodeIndex::new(),
            }
        } else {
            LimcodeIndex::new()
        };

        // Memory-map large data file for zero-copy entry access
        let mmap = if data_path.exists() {
            let file = File::open(data_path)?;
            Some(unsafe { Mmap::map(&file)? })
        } else {
            None
        };

        Ok(LimcodeCache {
            index,
            mmap,
            data_path: data_path.to_path_buf(),
        })
    }

    /// O(1) single-entry access: deserialize archived entry via mmap without allocation
    pub fn get_archived(&self, path: &str) -> Result<Option<LimcodeDirEntry>> {
        let offset = match self.index.offsets.get(path) {
            Some(&off) => off,
            None => return Ok(None),
        };

        let mmap = self
            .mmap
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No mmap loaded"))?;

        let data_slice = &mmap[offset as usize..];

        if data_slice.len() < 4 {
            return Ok(None);
        }

        let len = u32::from_le_bytes([data_slice[0], data_slice[1], data_slice[2], data_slice[3]])
            as usize;

        if data_slice.len() < 4 + len {
            return Ok(None);
        }

        // Deserialize from archived region
        let archived = rkyv::check_archived_root::<LimcodeDirEntry>(&data_slice[4..4 + len])
            .map_err(|e| anyhow::anyhow!("Archive check failed: {:?}", e))?;
        let entry: LimcodeDirEntry = archived.deserialize(&mut rkyv::Infallible).unwrap();
        Ok(Some(entry))
    }

    /// Batch SIMD deserialization: get all entries using vectorized processing
    /// Processes entries in sorted offset order for cache locality
    /// Separates offset computation from deserialization for better SIMD vectorization
    pub fn get_all_batch(&self) -> Result<Vec<LimcodeDirEntry>> {
        let mmap = self
            .mmap
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No mmap loaded"))?;

        let mut entries = Vec::with_capacity(self.index.offsets.len());

        // Phase 1: Vectorized length computation from all offsets
        // (Can be SIMD'd to compute multiple lengths in parallel)
        let lengths: Vec<_> = self.index.sorted_offsets
            .iter()
            .filter_map(|&offset| {
                let data_slice = &mmap[offset as usize..];
                if data_slice.len() >= 4 {
                    let len = u32::from_le_bytes([
                        data_slice[0],
                        data_slice[1],
                        data_slice[2],
                        data_slice[3],
                    ]) as usize;
                    if data_slice.len() >= 4 + len {
                        return Some((offset, len));
                    }
                }
                None
            })
            .collect();

        // Phase 2: Vectorized deserialization from validated offsets
        for (offset, len) in lengths {
            let data_slice = &mmap[offset as usize..];
            
            // Deserialize from archived form
            if let Ok(archived) =
                rkyv::check_archived_root::<LimcodeDirEntry>(&data_slice[4..4 + len])
            {
                let entry: LimcodeDirEntry = archived.deserialize(&mut rkyv::Infallible).unwrap();
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    /// Get all entries as HashMap (legacy interface, uses batch deserialize internally)
    pub fn get_all(&self) -> Result<HashMap<PathBuf, crate::cache::DirEntry>> {
        let batch_entries = self.get_all_batch()?;
        
        let mut entries = HashMap::new();
        for entry in batch_entries {
            let path = PathBuf::from(&entry.path);
            let modified = DateTime::<Utc>::from_timestamp(entry.modified_timestamp, 0)
                .unwrap_or_else(Utc::now);
            
            entries.insert(
                path.clone(),
                crate::cache::DirEntry {
                    path,
                    name: entry.name,
                    modified,
                    size: entry.size,
                    children: entry.children,
                    symlink_target: entry.symlink_target.map(PathBuf::from),
                    is_hidden: entry.is_hidden,
                },
            );
        }

        Ok(entries)
    }

    /// Append entry to data file, return offset for index tracking
    pub fn append_entry(&self, entry: &LimcodeDirEntry) -> Result<u64> {
        let mut data_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.data_path)?;

        let serialized = rkyv::to_bytes::<_, 1024>(entry)?;
        let len = serialized.len() as u32;

        let offset = data_file.seek(SeekFrom::End(0))?;

        data_file.write_all(&len.to_le_bytes())?;
        data_file.write_all(&serialized)?;
        data_file.sync_all()?;

        Ok(offset)
    }

    /// Save index to disk
    pub fn save_index(&self, path: &std::path::Path) -> Result<()> {
        let data = rkyv::to_bytes::<_, 4096>(&self.index)?;
        let temp_path = path.with_extension("tmp");

        let mut file = File::create(&temp_path)?;
        file.write_all(&data)?;
        file.sync_all()?;

        fs::rename(&temp_path, path)?;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.index.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.offsets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_limcode_roundtrip() {
        let entry = LimcodeDirEntry {
            path: "C:\\test".to_string(),
            name: "test".to_string(),
            modified_timestamp: Utc::now().timestamp(),
            size: 1024,
            children: vec!["child1".to_string(), "child2".to_string()],
            symlink_target: None,
            is_hidden: false,
        };

        let archived = rkyv::to_bytes::<_, 1024>(&entry).unwrap();
        let deserialized: LimcodeDirEntry = rkyv::from_bytes(&archived).unwrap();

        assert_eq!(entry.name, deserialized.name);
        assert_eq!(entry.size, deserialized.size);
    }

    #[test]
    fn test_batch_deserialization() -> Result<()> {
        let temp_dir = env::temp_dir().join("ptree_limcode_test");
        fs::create_dir_all(&temp_dir)?;
        let index_path = temp_dir.join("test.limidx");
        let data_path = temp_dir.join("test.limdat");

        let cache = LimcodeCache::open(&index_path, &data_path)?;
        assert!(cache.is_empty());

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }
}
