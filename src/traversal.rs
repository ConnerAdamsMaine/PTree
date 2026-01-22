use crate::cache::{DiskCache, DirEntry};
use crate::cli::Args;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, Duration};
use chrono::Utc;
use parking_lot::RwLock;
use anyhow::Result;



/// Debug timing information and statistics
#[derive(Debug, Clone)]
pub struct DebugInfo {
    pub is_first_run: bool,
    pub scan_root: PathBuf,
    pub cache_used: bool,
    pub traversal_time: Duration,
    pub save_time: Duration,
    pub cache_index_time: Duration,
    pub total_dirs: usize,
    pub total_files: usize,
    pub threads_used: usize,
}

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
    
    /// Directories that changed since last scan (for incremental updates)
    /// If set, only these directories will be rescanned; unset means full scan
    pub changed_dirs_filter: Option<std::collections::HashSet<String>>,

    /// Skip statistics: count of skipped directories (shared across threads)
    pub skip_stats: Arc<Mutex<std::collections::HashMap<String, usize>>>,
}

/// Traverse disk and update cache (per README spec)
///
/// Cache Correctness Model:
/// - First scan: Produces accurate snapshot of filesystem at scan time
/// - After scan, before USN refresh: Cache may lag live filesystem changes (eventual consistency)
/// - After 1-hour interval: USN Journal refresh synchronizes cache to current state
/// - Cache invariant: Always correct given sufficient time (1-hour refresh window)
///
/// USN Journal Management:
/// - Max size: 500MB (hardcoded)
/// - On wrap-around: Cache is increased up to 500MB limit
/// - If 500MB capacity is reached and wrap occurs: Automatic fallback to full rescan
/// - USN entries are cached; refresh interval is 1 hour from last cache write
///
/// Returns DebugInfo with timing information if --debug is enabled
///
/// Algorithm:
/// 1. On first run: Full scan of specified drive and cache results
/// 2. On subsequent runs: Check cache age and USN Journal
/// 3. If cache < 1 hour old: Use cache (instant return)
/// 4. If cache >= 1 hour old: Check USN Journal for wrap-around
/// 5. If wrap-around detected: Full rescan of specified drive
/// 6. Initialize work queue with drive root
/// 7. Spawn worker threads that process queue in parallel (iterative DFS)
/// 8. Flush all pending writes and save cache atomically
pub fn traverse_disk(drive: &char, cache: &mut DiskCache, args: &Args) -> Result<DebugInfo> {
    // Determine scan root: current directory by default, full drive with --force
    let scan_root = if args.force {
        // --force: scan full drive
        let root = PathBuf::from(format!("{}:\\", drive));
        if !root.exists() {
            anyhow::bail!("Drive {} does not exist", drive);
        }
        root
    } else {
        // Default: scan current directory and subdirectories
        std::env::current_dir()?
    };
    
    // Verify scan root exists and is a directory
    if !scan_root.exists() {
        anyhow::bail!("Scan root does not exist: {}", scan_root.display());
    }
    if !scan_root.is_dir() {
        anyhow::bail!("Scan root is not a directory: {}", scan_root.display());
    }

    let is_first_run = cache.entries.is_empty();
    cache.root = scan_root.clone();

    // Ensure root directory is added to cache (important for --no-cache mode)
    if is_first_run && !cache.entries.contains_key(&scan_root) {
        let root_entry = DirEntry {
            path: scan_root.clone(),
            name: scan_root
                .file_name()
                .and_then(|n| n.to_str().map(|s| s.to_string()))
                .unwrap_or_default(),
            modified: Utc::now(),
            size: 0,
            children: Vec::new(),
            symlink_target: None,
            is_hidden: false,
        };
        cache.entries.insert(scan_root.clone(), root_entry);
    }

    // ============================================================================
    // Check Cache Freshness (configurable via --cache-ttl, default 1 hour)
    // ============================================================================

    let cache_ttl_seconds = args.cache_ttl.unwrap_or(3600);
    
    let should_use_cache = if args.no_cache {
        false // --no-cache always triggers rescan
    } else if args.force {
        false // --force always triggers rescan
    } else if is_first_run {
        false // First run always scans
    } else {
        // Check cache freshness rule (time-based only)
        let now = Utc::now();
        let age = now.signed_duration_since(cache.last_scan);
        age.num_seconds() < cache_ttl_seconds as i64
    };
    
    if should_use_cache {
        let total_files = cache.entries.values().map(|e| e.children.len()).sum();
        return Ok(DebugInfo {
            is_first_run: false,
            scan_root: cache.root.clone(),
            cache_used: true,
            traversal_time: Duration::from_secs(0),
            save_time: Duration::from_secs(0),
            cache_index_time: Duration::from_secs(0),
            total_dirs: cache.entries.len(),
            total_files,
            threads_used: 0,
        });
    }

    // ============================================================================
    // Prepare for Traversal
    // ============================================================================
    
    // Note: Incremental filtering is now handled in main.rs via the --incremental flag
    // This allows cleaner separation between incremental (USN Journal) and full scan (DFS)
    let changed_dirs_filter: Option<std::collections::HashSet<String>> = None;

    // ============================================================================
    // Initialize Traversal State
    // ============================================================================

    let mut work_queue = VecDeque::new();
    work_queue.push_back(scan_root.clone());

    let state = TraversalState {
        work_queue: Arc::new(Mutex::new(work_queue)),
        cache: Arc::new(RwLock::new(cache.clone())),
        in_progress: Arc::new(Mutex::new(std::collections::HashSet::new())),
        skip_dirs: args.skip_dirs(),
        changed_dirs_filter,
        skip_stats: Arc::new(Mutex::new(std::collections::HashMap::new())),
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

    let traversal_start = Instant::now();
    let filter = state.changed_dirs_filter.clone();
    let root = scan_root.clone();
    let skip_stats_ref = Arc::clone(&state.skip_stats);
    pool.in_place_scope(|s| {
        for _ in 0..num_threads {
            let work = Arc::clone(&state.work_queue);
            let cache_ref = Arc::clone(&state.cache);
            let skip = state.skip_dirs.clone();
            let in_progress = Arc::clone(&state.in_progress);
            let filter_ref = filter.clone();
            let root_ref = root.clone();
            let stats_ref = Arc::clone(&skip_stats_ref);

            s.spawn(move |_| {
                dfs_worker(&work, &cache_ref, &skip, &in_progress, &filter_ref, &root_ref, &stats_ref);
            });
        }
    });
    let traversal_elapsed = traversal_start.elapsed();

    // ============================================================================
    // Extract & Save Final Cache
    // ============================================================================

    let mut final_cache = match Arc::try_unwrap(state.cache) {
        Ok(lock) => lock.into_inner(),
        Err(arc) => {
            let guard = arc.read();
            guard.clone()
        }
    };

    // Flush any remaining pending writes before saving
    final_cache.flush_pending_writes();

    let cache_index_start = Instant::now();
    
    *cache = final_cache;
    cache.last_scan = Utc::now();

    // Transfer skip statistics from traversal state to cache
    let skip_stats = match Arc::try_unwrap(state.skip_stats) {
        Ok(lock) => lock.into_inner().unwrap_or_default(),
        Err(arc) => {
            let guard = arc.lock().unwrap();
            guard.clone()
        }
    };
    cache.skip_stats = skip_stats;

    let cache_path = if let Some(ref custom_dir) = args.cache_dir {
        crate::cache::get_cache_path_custom(Some(custom_dir))?
    } else {
        crate::cache::get_cache_path()?
    };
    
    let save_start = Instant::now();
    if !args.no_cache {
        cache.save(&cache_path)?;
    }
    let save_elapsed = save_start.elapsed();
    
    let cache_index_elapsed = cache_index_start.elapsed();

    // ============================================================================
    // Return Debug Info
    // ============================================================================

    let total_files = cache.entries.values().map(|e| e.children.len()).sum();
    
    Ok(DebugInfo {
        is_first_run,
        scan_root: cache.root.clone(),
        cache_used: false,
        traversal_time: traversal_elapsed,
        save_time: save_elapsed,
        cache_index_time: cache_index_elapsed,
        total_dirs: cache.entries.len(),
        total_files,
        threads_used: num_threads,
    })
}

/// Worker thread for DFS traversal
///
/// Each worker thread:
/// 1. Pulls directories from shared work queue
/// 2. Acquires per-directory lock to prevent duplicate processing
/// 3. Enumerates directory, filters skipped entries
/// 4. For incremental updates: only process directories in changed_dirs_filter
/// 5. Buffers children in cache and queues directories for processing
fn dfs_worker(
    work_queue: &Arc<Mutex<VecDeque<PathBuf>>>,
    cache: &Arc<RwLock<DiskCache>>,
    skip_dirs: &std::collections::HashSet<String>,
    in_progress: &Arc<Mutex<std::collections::HashSet<PathBuf>>>,
    changed_dirs_filter: &Option<std::collections::HashSet<String>>,
    scan_root: &PathBuf,
    skip_stats: &Arc<Mutex<std::collections::HashMap<String, usize>>>,
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
                // Check Incremental Filter (if applicable)
                // ============================================================
                
                let should_process = if let Some(filter) = changed_dirs_filter {
                    // Incremental mode: only process if this directory changed
                    let dir_name = path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    filter.contains(dir_name) || path == *scan_root
                } else {
                    // Full scan mode: process all directories
                    true
                };
                
                if should_process {
                    // ============================================================
                    // Enumerate Directory & Process Entries
                    // ============================================================

                    if let Ok(entries) = fs::read_dir(&path) {
                         let mut children = Vec::new();
                         let mut child_entries = Vec::new();
                         let mut child_dirs_to_queue = Vec::new();
                         let mut child_files_to_cache = Vec::new();
                         let mut skipped = Vec::new(); // Batch skipped directories

                         for entry_result in entries {
                              if let Ok(entry) = entry_result {
                                  let file_name = entry.file_name();
                                  let file_name_str = file_name.to_string_lossy();

                                  // Skip filtered directories
                                  if should_skip(&file_name_str, skip_dirs) {
                                      // Batch skip statistics (don't lock on every skip)
                                      skipped.push(file_name_str.to_string());
                                      continue;
                                  }

                                  let child_path = entry.path();
                                  children.push(file_name_str.to_string());

                                  // Check if this is a directory (avoid unnecessary metadata calls for files)
                                  match entry.file_type() {
                                      Ok(ft) if ft.is_dir() => {
                                          // Queue directories for processing
                                          child_dirs_to_queue.push(child_path.clone());
                                          // Also add to cache for file listing
                                          if !child_files_to_cache.iter().any(|p| p == &child_path) {
                                              child_files_to_cache.push(child_path);
                                          }
                                      }
                                      Ok(ft) if ft.is_symlink() => {
                                          // Capture symlink target - add to both queues if it's a dir symlink
                                          let target = fs::read_link(&child_path).ok();
                                          child_entries.push((file_name_str.to_string(), target));
                                          child_files_to_cache.push(child_path.clone());
                                          // Don't queue symlinks for traversal - they would cause loops
                                      }
                                      Ok(_) => {
                                          // Regular file: add to cache but don't queue for traversal
                                          child_files_to_cache.push(child_path);
                                      }
                                      _ => {} // Couldn't get file type, skip
                                  }
                              }
                         }

                         // ========================================================
                         // Batch queue directories (reduce lock contention)
                         // ========================================================
                         if !child_dirs_to_queue.is_empty() {
                             let mut queue = work_queue.lock().unwrap();
                             for dir_path in child_dirs_to_queue {
                                 queue.push_back(dir_path);
                             }
                         }
                         
                         // ========================================================
                         // Batch cache file entries (for tree output)
                         // ========================================================
                         if !child_files_to_cache.is_empty() {
                             let mut cache_guard = cache.write();
                             for file_path in child_files_to_cache {
                                 let file_entry = DirEntry {
                                     path: file_path.clone(),
                                     name: file_path
                                         .file_name()
                                         .and_then(|n| n.to_str().map(|s| s.to_string()))
                                         .unwrap_or_default(),
                                     modified: Utc::now(),
                                     size: 0,
                                     children: Vec::new(), // Files don't have children
                                     symlink_target: None,
                                     is_hidden: false,
                                 };
                                 cache_guard.add_entry(file_path, file_entry);
                             }
                         } // Lock released immediately

                         // ========================================================
                         // Batch flush skip statistics (reduce lock contention)
                         // ========================================================
                         if !skipped.is_empty() {
                             let mut stats = skip_stats.lock().unwrap();
                             for skip_name in skipped {
                                 *stats.entry(skip_name).or_insert(0) += 1;
                             }
                         }

                         // ========================================================
                         // Skip sorting during traversal (defer to output phase)
                         // Children list stored unsorted for now
                         // ========================================================

                         // Check if directory has hidden attribute (Windows only)
                         let is_hidden = {
                             #[cfg(windows)]
                             {
                                 use std::os::windows::fs::MetadataExt;
                                 fs::metadata(&path)
                                     .map(|m| {
                                         const FILE_ATTRIBUTE_HIDDEN: u32 = 0x02;
                                         (m.file_attributes() & FILE_ATTRIBUTE_HIDDEN) != 0
                                     })
                                     .unwrap_or(false)
                             }
                             #[cfg(not(windows))]
                             {
                                 // Unix-like: check if name starts with dot
                                 path.file_name()
                                     .and_then(|n| n.to_str())
                                     .map(|s| s.starts_with('.'))
                                     .unwrap_or(false)
                             }
                         };

                         let dir_entry = DirEntry {
                             path: path.clone(),
                             name: path
                                 .file_name()
                                 .and_then(|n| n.to_str().map(|s| s.to_string()))
                                 .unwrap_or_default(),
                             modified: Utc::now(),
                             size: 0,
                             children, // Unsorted for now - sorted at output time
                             symlink_target: None, // Directories themselves are not symlinks
                             is_hidden,
                         };

                         // ========================================================
                         // Buffer entry in cache with automatic batching
                         // Flush happens automatically when threshold is reached
                         // ========================================================
                         {
                             let mut cache_guard = cache.write();
                             cache_guard.add_entry(path.clone(), dir_entry);
                         } // Lock released immediately
                    }

                    // ============================================================
                    // Release Per-Directory Lock
                    // ============================================================

                    {
                        let mut progress = in_progress.lock().unwrap();
                        progress.remove(&path);
                    }
                } else {
                    // Directory filtered out (incremental mode): skip it
                    {
                        let mut progress = in_progress.lock().unwrap();
                        progress.remove(&path);
                    }
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
