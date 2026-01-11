use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use anyhow::Result;
use serde_json::json;
use colored::Colorize;

/// Directory metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub path: PathBuf,
    pub name: String,
    pub modified: DateTime<Utc>,
    pub size: u64,
    pub children: Vec<String>, // child names only, not full paths
}

/// In-memory tree cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskCache {
    /// Map of absolute paths to directory entries
    pub entries: HashMap<PathBuf, DirEntry>,
    
    /// Last scan timestamp
    pub last_scan: DateTime<Utc>,
    
    /// Root path (e.g., C:\)
    pub root: PathBuf,
    
    /// Pending writes (buffered for batch updates)
    #[serde(skip)]
    pub pending_writes: Vec<(PathBuf, DirEntry)>,
    
    /// Maximum pending writes before flush
    #[serde(skip)]
    pub flush_threshold: usize,
}

impl DiskCache {
    // ============================================================================
    // Cache Loading & Saving
    // ============================================================================

    /// Open or create cache file
    pub fn open(path: &Path) -> Result<Self> {
        fs::create_dir_all(path.parent().unwrap())?;

        if path.exists() {
            let mut file = File::open(path)?;
            let mut data = Vec::new();
            file.read_to_end(&mut data)?;
            let mut cache: DiskCache = bincode::deserialize(&data)?;
            cache.pending_writes = Vec::new();
            cache.flush_threshold = 10000; // Flush every 10k entries
            Ok(cache)
        } else {
            Ok(DiskCache {
                entries: HashMap::new(),
                last_scan: Utc::now(),
                root: PathBuf::new(),
                pending_writes: Vec::new(),
                flush_threshold: 10000,
            })
        }
    }

    /// Save cache atomically using temp file + rename pattern
    pub fn save(&mut self, path: &Path) -> Result<()> {
        self.flush_pending_writes();

        let data = bincode::serialize(&self)?;
        let temp_path = path.with_extension("tmp");

        // Write to temporary file first
        let mut file = File::create(&temp_path)?;
        file.write_all(&data)?;
        file.sync_all()?;

        // Atomic rename (prevents corruption if crash occurs)
        fs::rename(&temp_path, path)?;
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

    /// Build ASCII tree output
    pub fn build_tree_output(&self) -> Result<String> {
        let mut output = String::new();

        if self.entries.is_empty() {
            return Ok("(empty)\n".to_string());
        }

        let root = &self.root;
        output.push_str(&format!("{}\n", root.display()));

        let mut visited = std::collections::HashSet::new();
        self.print_tree(&mut output, root, &mut visited, "", true)?;

        Ok(output)
    }

    fn print_tree(
        &self,
        output: &mut String,
        path: &Path,
        visited: &mut std::collections::HashSet<PathBuf>,
        prefix: &str,
        is_last: bool,
    ) -> Result<()> {
        if visited.contains(path) {
            return Ok(()); // Avoid cycles
        }
        visited.insert(path.to_path_buf());

        if let Some(entry) = self.get_entry(path) {
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
                output.push_str(&format!("{}{}{}\n", prefix, branch, child_name));

                let child_path = path.join(child_name);
                self.print_tree(
                    output,
                    &child_path,
                    visited,
                    &format!("{}{}", prefix, child_prefix),
                    is_last_child,
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
        let mut output = String::new();

        if self.entries.is_empty() {
            return Ok("(empty)\n".to_string());
        }

        let root = &self.root;
        output.push_str(&format!("{}\n", root.display().to_string().blue().bold()));

        let mut visited = std::collections::HashSet::new();
        self.print_colored_tree(&mut output, root, &mut visited, "", true)?;

        Ok(output)
    }

    fn print_colored_tree(
        &self,
        output: &mut String,
        path: &Path,
        visited: &mut std::collections::HashSet<PathBuf>,
        prefix: &str,
        is_last: bool,
    ) -> Result<()> {
        if visited.contains(path) {
            return Ok(()); // Avoid cycles
        }
        visited.insert(path.to_path_buf());

        if let Some(entry) = self.get_entry(path) {
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
                let name_colored = child_name.bright_blue().to_string();
                output.push_str(&format!("{}{}{}\n", prefix, branch_colored, name_colored));

                let child_path = path.join(child_name);
                self.print_colored_tree(
                    output,
                    &child_path,
                    visited,
                    &format!("{}{}", prefix, child_prefix),
                    is_last_child,
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
        let mut root_json = json!({
            "path": self.root.to_string_lossy().to_string(),
            "children": []
        });

        if self.entries.is_empty() {
            return Ok(root_json.to_string());
        }

        let mut visited = std::collections::HashSet::new();
        self.populate_json(&mut root_json, &self.root, &mut visited)?;

        Ok(serde_json::to_string_pretty(&root_json)?)
    }

    fn populate_json(
        &self,
        node: &mut serde_json::Value,
        path: &Path,
        visited: &mut std::collections::HashSet<PathBuf>,
    ) -> Result<()> {
        if visited.contains(path) {
            return Ok(());
        }
        visited.insert(path.to_path_buf());

        if let Some(entry) = self.get_entry(path) {
            let mut children_array = Vec::new();
            let mut children_names: Vec<_> = entry.children.iter().collect();
            children_names.sort();

            for child_name in children_names {
                let child_path = path.join(child_name);
                let mut child_json = json!({
                    "name": child_name,
                    "path": child_path.to_string_lossy().to_string(),
                    "children": []
                });

                self.populate_json(&mut child_json, &child_path, visited)?;
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
