// Incremental cache updates via USN Journal
// Applies file system changes to the cache without full rescans

use ptree_cache::DiskCache;
use anyhow::Result;

/// Attempt incremental cache update using USN Journal
///
/// Returns true if incremental update succeeded, false if should fall back to full scan
/// - If journal unavailable: Returns false and falls back to full scan
/// - If journal available: Applies changes and returns true
#[cfg(windows)]
pub fn try_incremental_update(
    _cache: &mut DiskCache,
    _drive_letter: char,
) -> Result<bool> {
    // USN Journal support is not implemented on this build
    // Fall back to full scan
    Ok(false)
}

#[cfg(not(windows))]
pub fn try_incremental_update(
    _cache: &mut DiskCache,
    _drive_letter: char,
) -> Result<bool> {
    Ok(false) // Not available on non-Windows
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
