use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Write, Seek, SeekFrom, Read};
use std::path::PathBuf;
use chrono::{DateTime, Utc};
use anyhow::Result;
use memmap2::Mmap;

#[cfg(windows)]
use ptree_driver::USNJournalState;

/// Serializable directory entry (serde-based for compatibility)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RkyvDirEntry {
    pub path: PathBuf,
    pub name: String,
    pub modified: DateTime<Utc>,
    pub size: u64,
    pub children: Vec<String>,
    pub symlink_target: Option<PathBuf>,
    pub is_hidden: bool,
}

/// Serializable cache index (serde-based for compatibility)
/// Maps paths → byte offsets, serialized separately for O(1) access
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RkyvCacheIndex {
    /// Offsets mapping for lazy single-node O(1) access
    pub offsets: HashMap<PathBuf, u64>,
    pub last_scan: DateTime<Utc>,
    pub root: PathBuf,
    pub last_scanned_root: PathBuf,
    #[cfg(windows)]
    pub usn_state: USNJournalState,
    pub skip_stats: HashMap<String, usize>,
}

impl RkyvCacheIndex {
    pub fn new() -> Self {
        RkyvCacheIndex {
            offsets: HashMap::new(),
            last_scan: Utc::now(),
            root: PathBuf::new(),
            last_scanned_root: PathBuf::new(),
            #[cfg(windows)]
            usn_state: USNJournalState::default(),
            skip_stats: HashMap::new(),
        }
    }
}

/// Memory-mapped cache using rkyv for zero-copy single-node O(1) access
///
/// Architecture:
/// - index file (.idx): contains RkyvCacheIndex (rkyv serialized)
/// - data file (.dat): contains rkyv-archived DirEntry objects at indexed offsets
///
/// Single-node access is O(1): load offset from index, deserialize from mmap in-place
/// No allocation or copying for field access during traversal
pub struct RkyvMmapCache {
    pub index: RkyvCacheIndex,
    mmap: Option<Mmap>,
    data_path: PathBuf,
}

impl RkyvMmapCache {
    /// Load cache from rkyv-serialized index and data files
    /// Index is fully deserialized (small), data is mmap'd (large, lazy access)
    pub fn open(index_path: &std::path::Path, data_path: &std::path::Path) -> Result<Self> {
        fs::create_dir_all(index_path.parent().unwrap())?;

        // Load index (small, safe to fully deserialize using serde)
         let index = if index_path.exists() {
             let mut file = File::open(index_path)?;
             let mut data = Vec::new();
             file.read_to_end(&mut data)?;
        
             // Deserialize index using serde bincode
             match bincode::deserialize::<RkyvCacheIndex>(&data) {
                 Ok(idx) => idx,
                 Err(_) => RkyvCacheIndex::new(),
             }
         } else {
             RkyvCacheIndex::new()
         };

        // Map data file (large, accessed lazily via O(1) offsets)
        let mmap = if data_path.exists() {
            let file = File::open(data_path)?;
            Some(unsafe { Mmap::map(&file)? })
        } else {
            None
        };

        Ok(RkyvMmapCache {
            index,
            mmap,
            data_path: data_path.to_path_buf(),
        })
    }

    /// O(1) lookup: get single directory entry via mmap offset
     /// Deserializes from mmap-backed binary data
     pub fn get_entry(&self, path: &std::path::Path) -> Result<Option<RkyvDirEntry>> {
         let offset = match self.index.offsets.get(path) {
             Some(&off) => off,
             None => return Ok(None),
         };
    
         let mmap = self
             .mmap
             .as_ref()
             .ok_or_else(|| anyhow::anyhow!("No mmap loaded"))?;
    
         let data_slice = &mmap[offset as usize..];
    
         // Read length prefix
         if data_slice.len() < 4 {
             return Ok(None);
         }
    
         let len = u32::from_le_bytes([data_slice[0], data_slice[1], data_slice[2], data_slice[3]])
             as usize;
    
         if data_slice.len() < 4 + len {
             return Ok(None);
         }
    
         // Deserialize entry from mmap'd region
         let entry: RkyvDirEntry = bincode::deserialize(&data_slice[4..4 + len])?;
         Ok(Some(entry))
     }
    
     /// Get all entries (full deserialization - only for batch operations or output)
     /// Used for tree building where we need owned data
     pub fn get_all(&self) -> Result<HashMap<PathBuf, crate::cache::DirEntry>> {
         let mut entries = HashMap::new();
    
         for path in self.index.offsets.keys() {
             if let Some(entry) = self.get_entry(path)? {
                 entries.insert(
                     entry.path.clone(),
                     crate::cache::DirEntry {
                         path: entry.path,
                         name: entry.name,
                         modified: entry.modified,
                         size: entry.size,
                         children: entry.children,
                         symlink_target: entry.symlink_target,
                         is_hidden: entry.is_hidden,
                     },
                 );
             }
         }
    
         Ok(entries)
     }

    /// Write bincode-serialized entry to data file
     /// Returns the offset where entry was written for index tracking
     pub fn append_entry(&self, entry: &RkyvDirEntry) -> Result<u64> {
         let mut data_file = std::fs::OpenOptions::new()
             .create(true)
             .append(true)
             .open(&self.data_path)?;
    
         let serialized = bincode::serialize(entry)?;
         let len = serialized.len() as u32;
    
         let offset = data_file.seek(SeekFrom::End(0))?;
    
         data_file.write_all(&len.to_le_bytes())?;
         data_file.write_all(&serialized)?;
         data_file.sync_all()?;
    
         Ok(offset)
     }
    
     /// Save index to disk (bincode serialized)
     pub fn save_index(&self, path: &std::path::Path) -> Result<()> {
         let data = bincode::serialize(&self.index)?;
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
    fn test_rkyv_roundtrip() -> Result<()> {
        let entry = RkyvDirEntry {
            path: PathBuf::from("C:\\test"),
            name: "test".to_string(),
            modified: Utc::now(),
            size: 1024,
            children: vec!["child1".to_string(), "child2".to_string()],
            symlink_target: None,
            is_hidden: false,
        };

        let archived = rkyv::to_bytes::<_, 1024>(&entry)?;
        let deserialized: RkyvDirEntry = rkyv::from_bytes(&archived)?;

        assert_eq!(entry.name, deserialized.name);
        assert_eq!(entry.size, deserialized.size);
        assert_eq!(entry.children.len(), deserialized.children.len());

        Ok(())
    }

    #[test]
    fn test_rkyv_cache_open() -> Result<()> {
        let temp_dir = env::temp_dir().join("ptree_rkyv_test");
        fs::create_dir_all(&temp_dir)?;
        let index_path = temp_dir.join("test.idx");
        let data_path = temp_dir.join("test.dat");

        let _cache = RkyvMmapCache::open(&index_path, &data_path)?;
        assert!(_cache.is_empty());

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }
}
