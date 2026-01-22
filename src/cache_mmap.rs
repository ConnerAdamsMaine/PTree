use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use anyhow::{Result, anyhow};
use memmap2::Mmap;

#[cfg(windows)]
use ptree_driver::USNJournalState;

use crate::cache::DirEntry;

/// Lightweight index mapping path offsets to byte positions in the mmap'd data file
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheIndex {
    /// Map of PathBuf to byte offset in the data file
    pub offsets: HashMap<PathBuf, u64>,
    
    /// Last scan timestamp
    pub last_scan: DateTime<Utc>,
    
    /// Root path
    pub root: PathBuf,
    
    /// Last scanned directory
    pub last_scanned_root: PathBuf,
    
    /// USN Journal state (Windows only)
    #[cfg(windows)]
    pub usn_state: USNJournalState,
    
    /// Skip statistics
    pub skip_stats: HashMap<String, usize>,
}

impl CacheIndex {
    pub fn new() -> Self {
        CacheIndex {
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

/// Memory-mapped cache system
/// 
/// Structure:
/// - index file: contains CacheIndex (paths → offsets)
/// - data file: contains serialized DirEntry objects at indexed offsets
pub struct MmapCache {
    /// Index mapping paths to byte offsets
    pub index: CacheIndex,
    
    /// Memory-mapped data file
    mmap: Option<Mmap>,
    
    /// Path to the data file (for lazy-loading entries)
    data_path: PathBuf,
    
    /// Buffer for pending writes before flush
    pub pending_writes: Vec<(PathBuf, DirEntry)>,
    
    /// Flush threshold
    pub flush_threshold: usize,
}

impl MmapCache {
    /// Load cache from index and data files
    pub fn open(index_path: &Path, data_path: &Path) -> Result<Self> {
        fs::create_dir_all(index_path.parent().unwrap())?;
        
        let index = if index_path.exists() {
            let mut file = File::open(index_path)?;
            let mut data = Vec::new();
            file.read_to_end(&mut data)?;
            bincode::deserialize(&data).unwrap_or_else(|_| CacheIndex::new())
        } else {
            CacheIndex::new()
        };
        
        let mmap = if data_path.exists() {
            let file = File::open(data_path)?;
            Some(unsafe { Mmap::map(&file)? })
        } else {
            None
        };
        
        Ok(MmapCache {
            index,
            mmap,
            data_path: data_path.to_path_buf(),
            pending_writes: Vec::new(),
            flush_threshold: 5000,
        })
    }
    
    /// Get a directory entry by path (deserializes from mmap'd region)
    pub fn get(&self, path: &Path) -> Result<Option<DirEntry>> {
        let offset = match self.index.offsets.get(path) {
            Some(&off) => off,
            None => return Ok(None),
        };
        
        let mmap = self.mmap.as_ref().ok_or_else(|| anyhow!("No mmap loaded"))?;
        let data_slice = &mmap[offset as usize..];
        
        // Deserialize single entry from this offset
        // Format: [4-byte length][serialized entry]
        if data_slice.len() < 4 {
            return Err(anyhow!("Invalid cache entry"));
        }
        
        let len = u32::from_le_bytes([
            data_slice[0],
            data_slice[1],
            data_slice[2],
            data_slice[3],
        ]) as usize;
        
        if data_slice.len() < 4 + len {
            return Err(anyhow!("Truncated cache entry"));
        }
        
        let entry: DirEntry = bincode::deserialize(&data_slice[4..4 + len])?;
        Ok(Some(entry))
    }
    
    /// Get all entries (loads entire mmap into memory - only for output generation)
    pub fn get_all(&self) -> Result<HashMap<PathBuf, DirEntry>> {
        let mut entries = HashMap::new();
        
        for path in self.index.offsets.keys() {
            if let Some(entry) = self.get(path)? {
                entries.insert(path.clone(), entry);
            }
        }
        
        Ok(entries)
    }
    
    /// Add a pending write
    pub fn add_entry(&mut self, path: PathBuf, entry: DirEntry) {
        self.pending_writes.push((path, entry));
        if self.pending_writes.len() >= self.flush_threshold {
            let _ = self.flush_pending_writes();
        }
    }
    
    /// Flush pending writes to disk
    pub fn flush_pending_writes(&mut self) -> Result<()> {
        if self.pending_writes.is_empty() {
            return Ok(());
        }
        
        // Open data file in append mode
        let mut data_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.data_path)?;
        
        for (path, entry) in self.pending_writes.drain(..) {
            let serialized = bincode::serialize(&entry)?;
            let len = serialized.len() as u32;
            
            // Record offset before writing
            let offset = data_file.seek(SeekFrom::End(0))?;
            self.index.offsets.insert(path, offset);
            
            // Write length + data
            data_file.write_all(&len.to_le_bytes())?;
            data_file.write_all(&serialized)?;
        }
        
        data_file.sync_all()?;
        
        // Reload mmap to include new data
        if let Ok(file) = File::open(&self.data_path) {
            self.mmap = Some(unsafe { Mmap::map(&file)? });
        }
        
        Ok(())
    }
    
    /// Save index to disk
    pub fn save_index(&self, path: &Path) -> Result<()> {
        let data = bincode::serialize(&self.index)?;
        let temp_path = path.with_extension("tmp");
        
        let mut file = File::create(&temp_path)?;
        file.write_all(&data)?;
        file.sync_all()?;
        
        fs::rename(&temp_path, path)?;
        Ok(())
    }
    
    /// Get number of cached entries
    pub fn len(&self) -> usize {
        self.index.offsets.len()
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.index.offsets.is_empty()
    }
}
