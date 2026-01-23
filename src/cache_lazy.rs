/// Lazy-loading cache using mmap for O(1) cold start
///
/// Architecture:
/// - Index (small, always loaded): PathBuf → offset mapping
/// - Data file (large, mmap'd): serialized entries at indexed offsets
/// - Entries: only deserialized on-demand during output phase
///
/// Benefits:
/// - Cold start: ~1ms (load index only)
/// - Hot access: O(1) per entry via mmap offset
/// - Memory: only entries in current build operation loaded
///
/// Files:
/// - .idx: bincode-serialized RkyvCacheIndex (pathbuf offsets)
/// - .dat: bincode-serialized RkyvDirEntry objects at indexed positions

use crate::cache::{DirEntry, USNJournalState};
use crate::cache_rkyv::{RkyvDirEntry, RkyvCacheIndex};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use anyhow::Result;
use memmap2::Mmap;

/// Lazy-loading cache wrapper
/// 
/// Keeps index in memory, data mmap'd, entries loaded on-demand
pub struct LazyCache {
    /// In-memory index: path → offset in data file
    pub index: RkyvCacheIndex,
    
    /// Memory-mapped data file (entries at offsets)
    mmap: Option<Mmap>,
    
    /// Path to data file
    data_path: PathBuf,
    
    /// LRU cache of recently loaded entries (configurable size)
    entry_cache: std::collections::VecDeque<(PathBuf, DirEntry)>,
    entry_cache_size: usize,
}

impl LazyCache {
    /// Open or create lazy cache
    /// Cold start: only loads index file (~ms scale for millions of entries)
    pub fn open(cache_path: &Path) -> Result<Self> {
        fs::create_dir_all(cache_path.parent().unwrap())?;
        
        let index_path = cache_path.with_extension("idx");
        let data_path = cache_path.with_extension("dat");
        
        // Load index (small, always in memory)
        let index = if index_path.exists() {
            let mut file = File::open(&index_path)?;
            let mut data = Vec::new();
            file.read_to_end(&mut data)?;
            bincode::deserialize::<RkyvCacheIndex>(&data)
                .unwrap_or_else(|_| RkyvCacheIndex::new())
        } else {
            RkyvCacheIndex::new()
        };
        
        // Memory-map data file (no deserialization)
        let mmap = if data_path.exists() && fs::metadata(&data_path)?.len() > 0 {
            let file = File::open(&data_path)?;
            Some(unsafe { Mmap::map(&file)? })
        } else {
            None
        };
        
        Ok(LazyCache {
            index,
            mmap,
            data_path,
            entry_cache: std::collections::VecDeque::with_capacity(1000),
            entry_cache_size: 1000,
        })
    }
    
    /// Load a single entry on-demand from mmap
    /// O(1) lookup + deserialization
    pub fn get_entry(&mut self, path: &Path) -> Result<Option<DirEntry>> {
        // Check LRU cache first
        if let Some(pos) = self.entry_cache.iter().position(|(p, _)| p == path) {
            let (_, entry) = self.entry_cache.remove(pos).unwrap();
            // Move to front (most recently used)
            self.entry_cache.push_front((path.to_path_buf(), entry.clone()));
            return Ok(Some(entry));
        }
        
        // Not in cache, load from mmap
        let offset = match self.index.offsets.get(path) {
            Some(&off) => off,
            None => return Ok(None),
        };
        
        let mmap = match self.mmap.as_ref() {
            Some(m) => m,
            None => return Ok(None),
        };
        
        let data_slice = &mmap[offset as usize..];
        
        // Read length prefix
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
        
        // Deserialize from mmap'd region
        let rkyv_entry: RkyvDirEntry = bincode::deserialize(&data_slice[4..4 + len])?;
        let entry = DirEntry {
            path: rkyv_entry.path,
            name: rkyv_entry.name,
            modified: rkyv_entry.modified,
            content_hash: rkyv_entry.content_hash,
            children: rkyv_entry.children,
            symlink_target: rkyv_entry.symlink_target,
            is_hidden: rkyv_entry.is_hidden,
            is_dir: rkyv_entry.is_dir,
        };
        
        // Add to LRU cache
        self.entry_cache.push_front((path.to_path_buf(), entry.clone()));
        if self.entry_cache.len() > self.entry_cache_size {
            self.entry_cache.pop_back();
        }
        
        Ok(Some(entry))
    }
    
    /// Get all entries from mmap (deferred to output phase)
    /// Still faster than loading from disk multiple times
    pub fn get_all(&mut self) -> Result<HashMap<PathBuf, DirEntry>> {
        let mut entries = HashMap::new();
        
        for path in self.index.offsets.keys() {
            if let Some(entry) = self.get_entry(path)? {
                entries.insert(path.clone(), entry);
            }
        }
        
        Ok(entries)
    }
    
    /// Save index to disk (fast atomic write)
    pub fn save_index(&self, cache_path: &Path) -> Result<()> {
        let index_path = cache_path.with_extension("idx");
        fs::create_dir_all(index_path.parent().unwrap())?;
        
        let data = bincode::serialize(&self.index)?;
        let temp_path = index_path.with_extension("tmp");
        
        let mut file = File::create(&temp_path)?;
        file.write_all(&data)?;
        file.sync_all()?;
        drop(file);
        
        fs::rename(&temp_path, &index_path)?;
        Ok(())
    }
    
    /// Append entry to data file (during traversal)
    /// Returns offset for index tracking
    pub fn append_entry(&self, entry: &DirEntry) -> Result<u64> {
        let rkyv_entry = RkyvDirEntry {
            path: entry.path.clone(),
            name: entry.name.clone(),
            modified: entry.modified,
            content_hash: entry.content_hash,
            children: entry.children.clone(),
            symlink_target: entry.symlink_target.clone(),
            is_hidden: entry.is_hidden,
            is_dir: entry.is_dir,
        };
        
        let mut data_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.data_path)?;
        
        let serialized = bincode::serialize(&rkyv_entry)?;
        let len = serialized.len() as u32;
        
        let offset = data_file.seek(SeekFrom::End(0))?;
        
        data_file.write_all(&len.to_le_bytes())?;
        data_file.write_all(&serialized)?;
        data_file.sync_all()?;
        
        Ok(offset)
    }
    
    /// Update index with new offsets (called after traversal)
    pub fn update_index(
        &mut self,
        offsets: HashMap<PathBuf, u64>,
        last_scan: DateTime<Utc>,
        root: PathBuf,
        last_scanned_root: PathBuf,
    ) {
        self.index.offsets = offsets;
        self.index.last_scan = last_scan;
        self.index.root = root;
        self.index.last_scanned_root = last_scanned_root;
    }
    
    #[cfg(windows)]
    pub fn set_usn_state(&mut self, usn_state: USNJournalState) {
        self.index.usn_state = usn_state;
    }
    
    pub fn set_skip_stats(&mut self, skip_stats: HashMap<String, usize>) {
        self.index.skip_stats = skip_stats;
    }
    
    /// Reload mmap after data file modifications
    pub fn reload_mmap(&mut self) -> Result<()> {
        if self.data_path.exists() && fs::metadata(&self.data_path)?.len() > 0 {
            let file = File::open(&self.data_path)?;
            self.mmap = Some(unsafe { Mmap::map(&file)? });
        }
        Ok(())
    }
    
    pub fn entry_count(&self) -> usize {
        self.index.offsets.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.index.offsets.is_empty()
    }
    
    pub fn last_scan(&self) -> DateTime<Utc> {
        self.index.last_scan
    }
    
    pub fn root(&self) -> &PathBuf {
        &self.index.root
    }
    
    pub fn skip_stats(&self) -> &HashMap<String, usize> {
        &self.index.skip_stats
    }
    
    #[cfg(windows)]
    pub fn usn_state(&self) -> &USNJournalState {
        &self.index.usn_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    
    #[test]
    fn test_lazy_cache_cold_start() -> Result<()> {
        let temp_dir = env::temp_dir().join("ptree_lazy_test");
        fs::create_dir_all(&temp_dir)?;
        let cache_path = temp_dir.join("test.cache");
        
        // Open (should be fast, no deserialization)
        let cache = LazyCache::open(&cache_path)?;
        assert!(cache.is_empty());
        
        fs::remove_dir_all(&temp_dir)?;
        Ok(())
    }
    
    #[test]
    fn test_lazy_cache_append_and_load() -> Result<()> {
        let temp_dir = env::temp_dir().join("ptree_lazy_append_test");
        fs::create_dir_all(&temp_dir)?;
        let cache_path = temp_dir.join("test.cache");
        
        let mut cache = LazyCache::open(&cache_path)?;
        
        let entry = DirEntry {
            path: PathBuf::from("C:\\test"),
            name: "test".to_string(),
            modified: Utc::now(),
            content_hash: 12345,
            children: vec!["child1".to_string()],
            symlink_target: None,
            is_hidden: false,
            is_dir: true,
        };
        
        let offset = cache.append_entry(&entry)?;
        cache.index.offsets.insert(entry.path.clone(), offset);
        cache.reload_mmap()?;
        
        // Load it back
        let loaded = cache.get_entry(&entry.path)?;
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().name, "test");
        
        fs::remove_dir_all(&temp_dir)?;
        Ok(())
    }
}
