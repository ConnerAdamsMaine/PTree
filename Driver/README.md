# ptree-driver: Windows File System Change Tracker

Real-time monitoring of file system changes via NTFS USN Journal for incremental cache updates to the ptree disk traversal tool.

## Overview

The driver monitors the NTFS Update Sequence Number (USN) Journal to detect file system changes in real-time. This allows ptree to maintain an up-to-date cache without full disk rescans.

## Architecture

### USN Journal Tracking (`usn_journal.rs`)

The core USN Journal implementation provides:

1. **Change Detection**: Monitors NTFS journal for file/directory modifications, creations, deletions, and renames
2. **State Management**: Tracks journal position via USN (Update Sequence Number)
3. **Windows API Integration**: Direct calls to `DeviceIoControl` for journal queries
4. **Cross-Platform Compatibility**: Gracefully degrades on non-Windows systems

### Key Types

#### `USNTracker`
Main interface for monitoring changes:

```rust
pub struct USNTracker {
    root: PathBuf,           // Volume root (e.g., C:\)
    state: USNJournalState,  // Persistent state
    buffer: Vec<u8>,         // 64KB buffer for records
}
```

**Methods:**
- `new(drive_letter, state)` - Create tracker for a drive
- `is_available()` - Check if journal is active
- `read_changes()` - Get new changes since last read
- `check_journal_validity()` - Detect journal resets
- `state()` / `set_state()` - Persist/restore position

#### `UsnRecord`
Individual change entry:

```rust
pub struct UsnRecord {
    path: PathBuf,           // Changed file/directory
    change_type: ChangeType, // Create/Modify/Delete/Rename/etc
    file_ref: u64,          // Stable file reference
    parent_ref: u64,        // Parent directory reference
    timestamp: DateTime<Utc>,
    usn: i64,               // Journal sequence number
    is_directory: bool,
}
```

#### `ChangeType`
Enum representing the nature of the change:
- `Created` - New file/directory
- `Modified` - Content or metadata changed
- `Deleted` - File/directory removed
- `Renamed` - Name changed
- `SecurityChanged` - Permissions/ACLs changed
- `PermissionsChanged` - Access rights changed
- `Other` - Other journal reason

#### `USNJournalState`
Persistent tracking state (for cache):

```rust
pub struct USNJournalState {
    last_usn: i64,          // Position in journal
    journal_id: u64,        // Journal identifier
    last_read: DateTime<Utc>, // When we last read
    drive_letter: char,     // Volume (C, D, E, etc)
    change_count: u64,      // Changes since last sync
}
```

## Windows API Integration

The driver uses Windows USN Journal APIs:

- **FSCTL_QUERY_USN_JOURNAL**: Get journal metadata (ID, size, bounds)
- **FSCTL_READ_USN_JOURNAL**: Read changes from a starting USN

These are called via `DeviceIoControl` on an open volume handle.

### Volume Handle

Volumes are opened with:
- `CreateFileW()` on path like `C:\`
- `GENERIC_READ` access
- `FILE_SHARE_READ` sharing
- No special attributes

### Record Parsing

USN records are binary structures (~100+ bytes each) containing:
- Fixed 60-byte header
- Variable-length filename (UTF-16, encoded at offset 56+)
- Reason flags indicating what changed

The driver parses these structures directly from the buffer.

## Usage

### Basic Usage

```rust
use ptree_driver::USNTracker;
use ptree_driver::USNJournalState;

// Create tracker for C: drive
let mut tracker = USNTracker::new('C', USNJournalState::default());

// Check if journal is available
if tracker.is_available()? {
    // Read all changes since last position
    let changes = tracker.read_changes()?;
    
    for record in changes {
        println!("{:?} - {:?}", record.path, record.change_type);
    }
}
```

### Persistence

Save state for incremental updates:

```rust
// Save current position
let state = tracker.state().clone();
serde_json::to_file("usn_state.json", &state)?;

// Load on next run
let state = serde_json::from_file("usn_state.json")?;
let mut tracker = USNTracker::new('C', state);

// Continues from where we left off
let new_changes = tracker.read_changes()?;
```

### Error Handling

Key error conditions:

- `JournalNotFound` - Journal not active (likely not NTFS)
- `InvalidHandle` - Can't open volume (needs admin)
- `Windows(msg)` - Low-level Windows API error
- `Parse(msg)` - Corrupted or unexpected record format

All are wrapped in `DriverResult<T>` for convenient error handling.

## Implementation Details

### Buffer Management

A 64KB buffer is pre-allocated and reused for each journal read. This is large enough for most change batches and avoids repeated allocations.

### Journal Validity

The journal ID can change if:
- The volume is dismounted/remounted
- Journal is deleted and recreated
- Some error conditions occur

The `check_journal_validity()` method detects this and resets to USN 0 to rescan everything.

### UTF-16 Filename Parsing

Filenames in the journal are UTF-16 LE encoded. The driver:
1. Reads bytes from the buffer at `filename_offset`
2. Converts pairs of bytes to u16 values
3. Uses `String::from_utf16_lossy()` for robust decoding

### Timestamp Conversion

Windows FILETIME is 100-nanosecond intervals since Jan 1, 1601. The driver:
1. Converts to Unix timestamp (seconds since 1970)
2. Uses chrono's `DateTime::from_timestamp()` for safe conversion
3. Falls back to `Utc::now()` for invalid timestamps

## Building

As part of the ptree workspace:

```bash
# Build just the driver library
cargo build -p ptree-driver

# Build driver binary
cargo build -p ptree-driver --bin ptree-driver

# Release build
cargo build --release -p ptree-driver
```

## Errors and Edge Cases

### Journal Not Available

Non-NTFS filesystems (FAT32, exFAT, etc.) don't have the USN Journal. The driver gracefully returns `JournalNotFound`.

### Permission Errors

Opening a volume handle requires administrator privileges. Standard users will get `AccessDenied`.

### Journal Full/Wrapped

If the journal is full and wraps around, you may miss some records. The journal ID change detection helps identify this.

### Buffer Overflows

If there are more changes than fit in 64KB, the driver returns what fits. Subsequent calls continue from the last USN read.

## Future Enhancements

1. **Journal Statistics**: Expose journal size, change rate, time range
2. **Filtering**: Only report certain change types
3. **Batch Operations**: Bulk change processing with callbacks
4. **Compression**: Compress state for long-term storage
5. **Multi-Volume**: Track changes across multiple drives simultaneously
6. **Performance Metrics**: Track change detection latency and throughput

## Testing

Unit tests are provided for:
- Change type classification
- Default state initialization
- Basic struct construction

Full integration tests (creating/deleting files and checking detection) are planned.

## References

- Microsoft: [Reading Entries from the Master File Table](https://learn.microsoft.com/en-us/windows/win32/fileio/change-journals)
- Microsoft: [FSCTL_READ_USN_JOURNAL](https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/fsctl-read-usn-journal)
- Microsoft: [FSCTL_QUERY_USN_JOURNAL](https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/fsctl-query-usn-journal)
