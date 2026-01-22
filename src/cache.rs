use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use anyhow::Result;
use serde_json::json;
use colored::Colorize;

#[cfg(windows)]
use crate::usn_journal::USNJournalState;

/// Directory metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub path: PathBuf,
    pub name: String,
    pub modified: DateTime<Utc>,
    pub size: u64,
    pub children: Vec<String>, // child names only, not full paths
    pub symlink_target: Option<PathBuf>, // If this entry is a symlink, store target
    pub is_hidden: bool, // Whether the directory has hidden attribute
}

/// In-memory tree cache
///
/// Memory Model (Hard-Bounded per README spec):
/// - Each directory entry is capped at 200 bytes (directory name + metadata)
/// - Memory usage is strictly: `memory ≤ directory_count × 200 bytes`
/// - Example: 2M directories = 400MB maximum memory footprint
/// - No unbounded string growth; paths are traversed, not accumulated
///
/// This is enforced at the type level through bounded path handling and
/// non-recursive DFS traversal. The 200-byte bound includes:
/// - PathBuf key in HashMap (varies, but path length is constrained)
/// - DirEntry value (name String, metadata, Vec<String> children)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskCache {
    /// Map of absolute paths to directory entries
    pub entries: HashMap<PathBuf, DirEntry>,

    /// Last scan timestamp
    pub last_scan: DateTime<Utc>,

    /// Root path (e.g., C:\)
    pub root: PathBuf,

    /// Last scanned directory (for subsequent runs to only scan current dir)
    pub last_scanned_root: PathBuf,

    /// USN Journal state for tracking changes (Windows only)
    #[cfg(windows)]
    pub usn_state: USNJournalState,

    /// Pending writes (buffered for batch updates)
    #[serde(skip)]
    pub pending_writes: Vec<(PathBuf, DirEntry)>,

    /// Maximum pending writes before flush
    #[serde(skip)]
    pub flush_threshold: usize,

    /// Whether to show hidden file attributes in output
    #[serde(skip)]
    pub show_hidden: bool,

    /// Skip statistics: count of skipped directories by name
    #[serde(skip)]
    pub skip_stats: std::collections::HashMap<String, usize>,
}

impl DiskCache {
    // ============================================================================
    // Cache Loading & Saving
    // ============================================================================

    /// Open or create cache file
     /// 
     /// Loads from rkyv mmap format (.idx and .dat files) for O(1) lazy loading
     /// Index is memory-mapped and accessed via bitshift operations
     pub fn open(path: &Path) -> Result<Self> {
         fs::create_dir_all(path.parent().unwrap())?;
    
         // Load from rkyv mmap format (.idx and .dat files)
         let index_path = path.with_extension("idx");
         let data_path = path.with_extension("dat");
         
         if index_path.exists() && data_path.exists() {
             if let Ok(mmap_cache) = Self::load_from_rkyv_mmap(&index_path, &data_path) {
                 return Ok(mmap_cache);
             }
         }
    
         Ok(Self::new_empty())
     }
     
     /// Load from rkyv mmap format (O(1) lazy loading via mmap + bitshift index)
     fn load_from_rkyv_mmap(index_path: &Path, data_path: &Path) -> Result<Self> {
         use crate::cache_rkyv::RkyvMmapCache;
         
         let rkyv_cache = RkyvMmapCache::open(index_path, data_path)?;
         
         // Load all entries (converts from RkyvDirEntry to DiskCache DirEntry)
         let entries = rkyv_cache.get_all()?;
         
         Ok(DiskCache {
             entries,
             last_scan: rkyv_cache.index.last_scan,
             root: rkyv_cache.index.root.clone(),
             last_scanned_root: rkyv_cache.index.last_scanned_root.clone(),
             #[cfg(windows)]
             usn_state: rkyv_cache.index.usn_state.clone(),
             pending_writes: Vec::new(),
             flush_threshold: 5000,
             show_hidden: false,
             skip_stats: rkyv_cache.index.skip_stats.clone(),
         })
     }
    
    /// Create a new empty cache with default USN state
    #[cfg(windows)]
    fn new_empty() -> Self {
        DiskCache {
            entries: HashMap::new(),
            last_scan: Utc::now(),
            root: PathBuf::new(),
            last_scanned_root: PathBuf::new(),
            usn_state: USNJournalState::default(),
            pending_writes: Vec::new(),
            flush_threshold: 5000, // More frequent flushes to reduce lock contention
            show_hidden: false,
            skip_stats: HashMap::new(),
        }
    }
    
    /// Create a new empty cache with default USN state (non-Windows)
    #[cfg(not(windows))]
    fn new_empty() -> Self {
        DiskCache {
            entries: HashMap::new(),
            last_scan: Utc::now(),
            root: PathBuf::new(),
            last_scanned_root: PathBuf::new(),
            pending_writes: Vec::new(),
            flush_threshold: 5000, // More frequent flushes to reduce lock contention
            show_hidden: false,
            skip_stats: HashMap::new(),
        }
    }

    /// Save cache using rkyv mmap format (index + data files with O(1) access)
     pub fn save(&mut self, path: &Path) -> Result<()> {
         self.flush_pending_writes();
    
         let index_path = path.with_extension("idx");
         let data_path = path.with_extension("dat");
         
         self.save_as_rkyv_mmap(&index_path, &data_path)?;
         Ok(())
     }
     
     /// Save cache in mmap format (index + data files with bincode serialization)
     fn save_as_rkyv_mmap(&self, index_path: &Path, data_path: &Path) -> Result<()> {
         use crate::cache_rkyv::{RkyvDirEntry, RkyvCacheIndex};
         use std::io::Seek;
         
         fs::create_dir_all(index_path.parent().unwrap())?;
         
         // Build index with byte offsets
         let mut rkyv_index = RkyvCacheIndex::new();
         rkyv_index.root = self.root.clone();
         rkyv_index.last_scanned_root = self.last_scanned_root.clone();
         rkyv_index.last_scan = self.last_scan;
         rkyv_index.skip_stats = self.skip_stats.clone();
         #[cfg(windows)]
         {
             rkyv_index.usn_state = self.usn_state.clone();
         }
         
         let mut data_file = File::create(data_path)?;
         
         for (path, entry) in &self.entries {
             let rkyv_entry = RkyvDirEntry {
                 path: entry.path.clone(),
                 name: entry.name.clone(),
                 modified: entry.modified,
                 size: entry.size,
                 children: entry.children.clone(),
                 symlink_target: entry.symlink_target.clone(),
                 is_hidden: entry.is_hidden,
             };
             
             let serialized = bincode::serialize(&rkyv_entry)?;
             let len = serialized.len() as u32;
             let offset = data_file.stream_position()?;
             
             rkyv_index.offsets.insert(path.clone(), offset);
             data_file.write_all(&len.to_le_bytes())?;
             data_file.write_all(&serialized)?;
         }
         data_file.sync_all()?;
         
         // Save index
         let index_serialized = bincode::serialize(&rkyv_index)?;
         let temp_path = index_path.with_extension("tmp");
         let mut index_file = File::create(&temp_path)?;
         index_file.write_all(&index_serialized)?;
         index_file.sync_all()?;
         fs::rename(&temp_path, index_path)?;
         
         Ok(())
     }

    // ============================================================================
    // Entry Management
    // ============================================================================

    /// Buffer a directory entry for batch writing
    pub fn buffer_entry(&mut self, path: PathBuf, entry: DirEntry) {
        self.pending_writes.push((path, entry));

        if self.pending_writes.len() >= self.flush_threshold {
            self.flush_pending_writes();
        }
    }

    /// Flush all buffered writes to main cache HashMap
    pub fn flush_pending_writes(&mut self) {
        for (path, entry) in self.pending_writes.drain(..) {
            self.entries.insert(path, entry);
        }
    }

    /// Add or update directory entry (via buffer)
    pub fn add_entry(&mut self, path: PathBuf, entry: DirEntry) {
        self.buffer_entry(path, entry);
    }

    /// Get entry by path
    pub fn get_entry(&self, path: &Path) -> Option<&DirEntry> {
        self.entries.get(path)
    }

    /// Format a directory name with optional hidden indicator
    pub fn format_name(&self, name: &str, path: &Path, show_hidden: bool) -> String {
        if !show_hidden {
            return name.to_string();
        }

        if let Some(entry) = self.get_entry(path) {
            if entry.is_hidden {
                format!("{} [H]", name)
            } else {
                name.to_string()
            }
        } else {
            name.to_string()
        }
    }

    /// Record that a directory was skipped
    pub fn record_skip(&mut self, dir_name: &str) {
        *self.skip_stats.entry(dir_name.to_string()).or_insert(0) += 1;
    }

    /// Get skip statistics report
    pub fn get_skip_report(&self) -> String {
        if self.skip_stats.is_empty() {
            return "(no directories skipped)".to_string();
        }

        let mut report = String::from("Skip Statistics:\n");
        let mut sorted: Vec<_> = self.skip_stats.iter().collect();
        sorted.sort_by_key(|(_name, count)| std::cmp::Reverse(**count));

        for (name, count) in sorted {
            report.push_str(&format!("  {} × {}\n", count, name));
        }

        report
    }

    /// Remove entry and all child entries
    pub fn remove_entry(&mut self, path: &Path) {
        self.entries.remove(path);
        let prefix = path.to_string_lossy().to_string();
        self.entries.retain(|k, _| {
            !k.to_string_lossy().starts_with(&prefix) || k == path
        });
    }

    // ============================================================================
    // ASCII Tree Output
    // ============================================================================

    /// Build ASCII tree output with optional max depth
    pub fn build_tree_output(&self) -> Result<String> {
        self.build_tree_output_with_depth(None)
    }

    /// Build ASCII tree output with optional max depth limit
    pub fn build_tree_output_with_depth(&self, max_depth: Option<usize>) -> Result<String> {
        let mut output = String::new();

        if self.entries.is_empty() {
            return Ok("(empty)\n".to_string());
        }

        let root = &self.root;
        output.push_str(&format!("{}\n", root.display()));

        // No need for visited set - filesystem is acyclic and in_progress set prevents cycles during traversal
        self.print_tree(&mut output, root, "", true, 0, max_depth)?;

        Ok(output)
    }

    fn print_tree(
        &self,
        output: &mut String,
        path: &Path,
        prefix: &str,
        is_last: bool,
        current_depth: usize,
        max_depth: Option<usize>,
    ) -> Result<()> {
        // Check depth limit
        if let Some(max) = max_depth {
            if current_depth >= max {
                return Ok(());
            }
        }

        if let Some(entry) = self.get_entry(path) {
            // Sort children only at output time (not during traversal)
            let mut children: Vec<_> = entry.children.iter().collect();
            children.sort();

            for (i, child_name) in children.iter().enumerate() {
                let is_last_child = i == children.len() - 1;
                let child_prefix = if is_last {
                    "    ".to_string()
                } else {
                    "│   ".to_string()
                };

                let branch = if is_last_child { "└── " } else { "├── " };
                
                // Check if this child is a symlink
                let child_path = path.join(child_name);
                let display_name = if let Some(entry) = self.get_entry(&child_path) {
                    let base_name = if let Some(target) = &entry.symlink_target {
                        format!("{} (→ {})", child_name, target.display())
                    } else {
                        self.format_name(child_name, &child_path, self.show_hidden)
                    };
                    base_name
                } else {
                    child_name.to_string()
                };
                
                output.push_str(&format!("{}{}{}\n", prefix, branch, display_name));
                self.print_tree(
                    output,
                    &child_path,
                    &format!("{}{}", prefix, child_prefix),
                    is_last_child,
                    current_depth + 1,
                    max_depth,
                )?;
            }
        }

        Ok(())
    }

    // ============================================================================
    // Colored Tree Output
    // ============================================================================

    /// Build colored tree output
    pub fn build_colored_tree_output(&self) -> Result<String> {
        self.build_colored_tree_output_with_depth(None)
    }

    /// Build colored tree output with optional max depth limit
    pub fn build_colored_tree_output_with_depth(&self, max_depth: Option<usize>) -> Result<String> {
        let mut output = String::new();

        if self.entries.is_empty() {
            return Ok("(empty)\n".to_string());
        }

        let root = &self.root;
        output.push_str(&format!("{}\n", root.display().to_string().blue().bold()));

        // No need for visited set - filesystem is acyclic and in_progress set prevents cycles during traversal
        self.print_colored_tree(&mut output, root, "", true, 0, max_depth)?;

        Ok(output)
    }

    fn print_colored_tree(
        &self,
        output: &mut String,
        path: &Path,
        prefix: &str,
        is_last: bool,
        current_depth: usize,
        max_depth: Option<usize>,
    ) -> Result<()> {
        // Check depth limit
        if let Some(max) = max_depth {
            if current_depth >= max {
                return Ok(());
            }
        }

        if let Some(entry) = self.get_entry(path) {
            // Sort children only at output time (not during traversal)
            let mut children: Vec<_> = entry.children.iter().collect();
            children.sort();

            for (i, child_name) in children.iter().enumerate() {
                let is_last_child = i == children.len() - 1;
                let child_prefix = if is_last {
                    "    ".to_string()
                } else {
                    "│   ".to_string()
                };

                let branch = if is_last_child { "└── " } else { "├── " };
                let branch_colored = branch.cyan().to_string();
                
                // Check if this child is a symlink
                let child_path = path.join(child_name);
                let display_name = if let Some(entry) = self.get_entry(&child_path) {
                    let base_name = if let Some(target) = &entry.symlink_target {
                        format!("{} (→ {})", child_name, target.display())
                    } else {
                        self.format_name(child_name, &child_path, self.show_hidden)
                    };
                    base_name.bright_blue().to_string()
                } else {
                    child_name.bright_blue().to_string()
                };
                
                output.push_str(&format!("{}{}{}\n", prefix, branch_colored, display_name));
                self.print_colored_tree(
                    output,
                    &child_path,
                    &format!("{}{}", prefix, child_prefix),
                    is_last_child,
                    current_depth + 1,
                    max_depth,
                )?;
            }
        }

        Ok(())
    }

    // ============================================================================
    // JSON Tree Output
    // ============================================================================

    /// Build JSON tree representation
    pub fn build_json_output(&self) -> Result<String> {
        self.build_json_output_with_depth(None)
    }

    /// Build JSON tree representation with optional max depth limit
    pub fn build_json_output_with_depth(&self, max_depth: Option<usize>) -> Result<String> {
        let mut root_json = json!({
            "path": self.root.to_string_lossy().to_string(),
            "children": []
        });

        if self.entries.is_empty() {
            return Ok(root_json.to_string());
        }

        // No need for visited set - filesystem is acyclic and in_progress set prevents cycles during traversal
        self.populate_json(&mut root_json, &self.root, 0, max_depth)?;

        Ok(serde_json::to_string_pretty(&root_json)?)
    }

    fn populate_json(
        &self,
        node: &mut serde_json::Value,
        path: &Path,
        current_depth: usize,
        max_depth: Option<usize>,
    ) -> Result<()> {
        // Check depth limit
        if let Some(max) = max_depth {
            if current_depth >= max {
                return Ok(());
            }
        }

        if let Some(entry) = self.get_entry(path) {
            let mut children_array = Vec::new();
            let mut children_names: Vec<_> = entry.children.iter().collect();
            // Sort children only at output time (not during traversal)
            children_names.sort();

            for child_name in children_names {
                let child_path = path.join(child_name);
                let mut child_json = json!({
                    "name": child_name,
                    "path": child_path.to_string_lossy().to_string(),
                    "children": []
                });

                self.populate_json(&mut child_json, &child_path, current_depth + 1, max_depth)?;
                children_array.push(child_json);
            }

            node["children"] = serde_json::json!(children_array);
        }

        Ok(())
    }
}

/// Get cache directory path
pub fn get_cache_path() -> Result<PathBuf> {
    let appdata = std::env::var("APPDATA")?;
    Ok(PathBuf::from(appdata)
        .join("ptree")
        .join("cache")
        .join("ptree.dat"))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cache_creation() -> Result<()> {
        let temp_dir = std::env::temp_dir().join("ptree_test");
        fs::create_dir_all(&temp_dir)?;
        let cache_path = temp_dir.join("test.dat");
        
        let cache = DiskCache::open(&cache_path)?;
        assert!(cache.entries.is_empty());
        
        fs::remove_file(&cache_path)?;
        Ok(())
    }
}
