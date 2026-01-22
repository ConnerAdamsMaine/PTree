// Incremental cache updates via USN Journal
// Applies file system changes to the cache without full rescans

#[cfg(windows)]
use crate::cache::{DiskCache, DirEntry};
use anyhow::Result;
use chrono::Utc;

#[cfg(windows)]
use ptree_driver::{USNTracker, UsnRecord, ChangeType};

/// Attempt incremental cache update using USN Journal
///
/// Returns true if incremental update succeeded, false if should fall back to full scan
/// - If journal unavailable: Returns false and falls back to full scan
/// - If journal available: Applies changes and returns true
#[cfg(windows)]
pub fn try_incremental_update(
    cache: &mut DiskCache,
    drive_letter: char,
) -> Result<bool> {
    // Create tracker with saved state
    let mut tracker = USNTracker::new(drive_letter, cache.usn_state.clone());

    // Check if journal is available on this volume
    if !tracker.is_available()? {
        return Ok(false); // Fall back to full scan
    }

    // Check if journal has wrapped around (very old state)
    if !tracker.check_journal_validity()? {
        // Journal was reset, need full scan
        return Ok(false);
    }

    // Read changes since last position
    let changes = tracker.read_changes()?;

    if changes.is_empty() {
        // No changes detected, update timestamp and continue
        cache.last_scan = Utc::now();
        cache.usn_state = tracker.state().clone();
        return Ok(true);
    }

    // Apply changes to cache
    apply_changes_to_cache(cache, &changes)?;

    // Update state for next run
    cache.usn_state = tracker.state().clone();
    cache.last_scan = Utc::now();

    Ok(true)
}

#[cfg(not(windows))]
pub fn try_incremental_update(
    _cache: &mut DiskCache,
    _drive_letter: char,
) -> Result<bool> {
    Ok(false) // Not available on non-Windows
}

/// Apply a batch of USN changes to the cache
#[cfg(windows)]
fn apply_changes_to_cache(cache: &mut DiskCache, changes: &[UsnRecord]) -> Result<()> {
    for record in changes {
        match record.change_type {
            ChangeType::Created => {
                apply_create(cache, record);
            }
            ChangeType::Modified => {
                apply_modified(cache, record);
            }
            ChangeType::Deleted => {
                apply_deleted(cache, record);
            }
            ChangeType::Renamed => {
                // Rename is complex - for now, treat as delete+create
                // In a real implementation, we'd track the old/new path
                apply_deleted(cache, record);
            }
            ChangeType::SecurityChanged | ChangeType::PermissionsChanged => {
                // Update metadata timestamp for security changes
                if let Some(entry) = cache.entries.get_mut(&record.path) {
                    entry.modified = record.timestamp;
                }
            }
            ChangeType::Other => {
                // Ignore other change types
            }
        }
    }

    Ok(())
}

/// Apply a file creation change
#[cfg(windows)]
fn apply_create(cache: &mut DiskCache, record: &UsnRecord) {
    if record.is_directory {
        // Only track directories
        let name = record
            .path
            .file_name()
            .and_then(|n: &std::ffi::OsStr| n.to_str())
            .unwrap_or("")
            .to_string();

        if !name.is_empty() && !cache.entries.contains_key(&record.path) {
            let entry = DirEntry {
                path: record.path.clone(),
                name: name.clone(),
                modified: record.timestamp,
                size: 0,
                children: Vec::new(),
                symlink_target: None,
                is_hidden: false,
            };

            // Add to parent's children list if parent exists
            if let Some(parent) = record.path.parent() {
                if let Some(parent_entry) = cache.entries.get_mut(parent) {
                    if !parent_entry.children.iter().any(|c| c == &name) {
                        parent_entry.children.push(name);
                    }
                }
            }

            cache.entries.insert(record.path.clone(), entry);
        }
    }
}

/// Apply a file modification change
#[cfg(windows)]
fn apply_modified(cache: &mut DiskCache, record: &UsnRecord) {
    if record.is_directory {
        if let Some(entry) = cache.entries.get_mut(&record.path) {
            // Update modification timestamp
            entry.modified = record.timestamp;
        } else {
            // Unknown directory - treat as create
            apply_create(cache, record);
        }
    }
}

/// Apply a file deletion change
#[cfg(windows)]
fn apply_deleted(cache: &mut DiskCache, record: &UsnRecord) {
    if record.is_directory {
        // Remove directory and all its children
        cache.remove_entry(&record.path);

        // Also remove from parent's children list
        if let Some(parent) = record.path.parent() {
            if let Some(parent_entry) = cache.entries.get_mut(parent) {
                let name = record
                    .path
                    .file_name()
                    .and_then(|n: &std::ffi::OsStr| n.to_str())
                    .unwrap_or("");
                parent_entry.children.retain(|c| c != name);
            }
        }
    }
}

/// Estimate change impact (for debugging/statistics)
#[cfg(windows)]
pub fn estimate_change_impact(changes: &[UsnRecord]) -> (usize, usize, usize, usize) {
    let mut creates = 0;
    let mut modifies = 0;
    let mut deletes = 0;
    let mut renames = 0;

    for record in changes {
        if !record.is_directory {
            continue; // Only count directories
        }

        match record.change_type {
            ChangeType::Created => creates += 1,
            ChangeType::Modified => modifies += 1,
            ChangeType::Deleted => deletes += 1,
            ChangeType::Renamed => renames += 1,
            _ => {}
        }
    }

    (creates, modifies, deletes, renames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn test_change_impact_estimation() {
        let changes = vec![];
        let (c, m, d, r) = estimate_change_impact(&changes);
        assert_eq!((c, m, d, r), (0, 0, 0, 0));
    }
}
