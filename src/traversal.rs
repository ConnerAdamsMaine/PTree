use crate::cache::{DiskCache, DirEntry};
use crate::cli::Args;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use chrono::Utc;
use parking_lot::RwLock;
use anyhow::Result;

/// Shared state for parallel DFS traversal across worker threads
pub struct TraversalState {
    /// Work queue: directories to be processed
    pub work_queue: Arc<Mutex<VecDeque<PathBuf>>>,

    /// Shared cache across all worker threads
    pub cache: Arc<RwLock<DiskCache>>,

    /// Track directories currently being processed (prevents duplicates)
    pub in_progress: Arc<Mutex<std::collections::HashSet<PathBuf>>>,

    /// Directories to skip during traversal
    pub skip_dirs: std::collections::HashSet<String>,
}

/// Traverse disk and update cache
///
/// Algorithm:
/// 1. Check cache freshness (< 1 hour). If fresh and not forced, return early.
/// 2. Initialize work queue with root directory
/// 3. Spawn worker threads that process queue in parallel (DFS)
/// 4. Flush all pending writes and save cache atomically
pub fn traverse_disk(drive: &char, cache: &mut DiskCache, args: &Args) -> Result<()> {
    let root = PathBuf::from(format!("{}:\\", drive));

    if !root.exists() {
        anyhow::bail!("Drive {} does not exist", drive);
    }

    cache.root = root.clone();

    // ============================================================================
    // Check Cache Freshness
    // ============================================================================

    if !args.force {
        let now = Utc::now();
        let age = now.signed_duration_since(cache.last_scan);
        if age.num_seconds() < 3600 && !cache.entries.is_empty() {
            return Ok(()); // Cache is fresh, skip rescan
        }
    }

    // ============================================================================
    // Initialize Traversal State
    // ============================================================================

    let mut work_queue = VecDeque::new();
    work_queue.push_back(root.clone());

    let state = TraversalState {
        work_queue: Arc::new(Mutex::new(work_queue)),
        cache: Arc::new(RwLock::new(cache.clone())),
        in_progress: Arc::new(Mutex::new(std::collections::HashSet::new())),
        skip_dirs: args.skip_dirs(),
    };

    // ============================================================================
    // Create Thread Pool & Determine Thread Count
    // ============================================================================

    let num_threads = args.threads.unwrap_or_else(|| num_cpus::get() * 2);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()?;

    // ============================================================================
    // Spawn Worker Threads for Parallel DFS Traversal
    // ============================================================================

    pool.in_place_scope(|s| {
        for _ in 0..num_threads {
            let work = Arc::clone(&state.work_queue);
            let cache_ref = Arc::clone(&state.cache);
            let skip = state.skip_dirs.clone();
            let in_progress = Arc::clone(&state.in_progress);

            s.spawn(move |_| {
                dfs_worker(&work, &cache_ref, &skip, &in_progress);
            });
        }
    });

    // ============================================================================
    // Extract & Save Final Cache
    // ============================================================================

    let final_cache = match Arc::try_unwrap(state.cache) {
        Ok(lock) => lock.into_inner(),
        Err(arc) => {
            let guard = arc.read();
            guard.clone()
        }
    };

    *cache = final_cache;
    cache.last_scan = Utc::now();

    let cache_path = crate::cache::get_cache_path()?;
    cache.save(&cache_path)?;

    Ok(())
}

/// Worker thread for DFS traversal
///
/// Each worker thread:
/// 1. Pulls directories from shared work queue
/// 2. Acquires per-directory lock to prevent duplicate processing
/// 3. Enumerates directory, filters skipped entries
/// 4. Buffers children in cache and queues directories for processing
fn dfs_worker(
    work_queue: &Arc<Mutex<VecDeque<PathBuf>>>,
    cache: &Arc<RwLock<DiskCache>>,
    skip_dirs: &std::collections::HashSet<String>,
    in_progress: &Arc<Mutex<std::collections::HashSet<PathBuf>>>,
) {
    loop {
        // ====================================================================
        // Get Next Directory From Work Queue
        // ====================================================================

        let dir_path = {
            let mut queue = work_queue.lock().unwrap();
            queue.pop_front()
        };

        if let Some(path) = dir_path {
            // ================================================================
            // Acquire Per-Directory Lock (prevents duplicate processing)
            // ================================================================

            let acquired = {
                let mut progress = in_progress.lock().unwrap();
                if !progress.contains(&path) {
                    progress.insert(path.clone());
                    true
                } else {
                    false
                }
            };

            if acquired {
                // ============================================================
                // Enumerate Directory & Process Entries
                // ============================================================

                if let Ok(entries) = fs::read_dir(&path) {
                    let mut children = Vec::new();

                    for entry_result in entries {
                        if let Ok(entry) = entry_result {
                            let file_name = entry.file_name();
                            let file_name_str = file_name.to_string_lossy();

                            // Skip filtered directories
                            if should_skip(&file_name_str, skip_dirs) {
                                continue;
                            }

                            let child_path = entry.path();
                            children.push(file_name_str.to_string());

                            // Queue directories for processing (avoid symlinks)
                            if let Ok(metadata) = entry.metadata() {
                                if metadata.is_dir() && !metadata.is_symlink() {
                                    let mut queue = work_queue.lock().unwrap();
                                    queue.push_back(child_path);
                                }
                            }
                        }
                    }

                    // ========================================================
                    // Sort Children (parallel for large directories)
                    // ========================================================

                    let sorted_children = if children.len() > 100 {
                        use rayon::slice::ParallelSliceMut;
                        let mut child_copy = children;
                        child_copy.par_sort();
                        child_copy
                    } else {
                        children.sort();
                        children
                    };

                    // ========================================================
                    // Buffer Directory Entry to Cache
                    // ========================================================

                    let dir_entry = DirEntry {
                        path: path.clone(),
                        name: path
                            .file_name()
                            .and_then(|n| n.to_str().map(|s| s.to_string()))
                            .unwrap_or_default(),
                        modified: Utc::now(),
                        size: 0,
                        children: sorted_children,
                    };

                    let mut cache_guard = cache.write();
                    cache_guard.add_entry(path.clone(), dir_entry);
                }

                // ============================================================
                // Release Per-Directory Lock
                // ============================================================

                {
                    let mut progress = in_progress.lock().unwrap();
                    progress.remove(&path);
                }
            }
        } else {
            // No more work in queue - worker can exit
            break;
        }
    }
}

fn should_skip(name: &str, skip_dirs: &std::collections::HashSet<String>) -> bool {
    skip_dirs.iter().any(|skip| {
        name.eq_ignore_ascii_case(skip)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_should_skip() {
        let mut skip = std::collections::HashSet::new();
        skip.insert("System32".to_string());
        skip.insert(".git".to_string());
        
        assert!(should_skip("System32", &skip));
        assert!(should_skip(".git", &skip));
        assert!(!should_skip("Documents", &skip));
    }
}
