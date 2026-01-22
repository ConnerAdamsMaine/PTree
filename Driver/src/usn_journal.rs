use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use chrono::{DateTime, Utc};
use crate::error::{DriverError, DriverResult};

#[cfg(windows)]
use std::mem;
#[cfg(windows)]
use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
#[cfg(windows)]
use winapi::um::winnt::{FILE_SHARE_READ, GENERIC_READ};
#[cfg(windows)]
use winapi::shared::minwindef::FALSE;
#[cfg(windows)]
use winapi::um::handleapi::{INVALID_HANDLE_VALUE, CloseHandle};
#[cfg(windows)]
use winapi::ctypes::c_void;

// ============================================================================
// Change Record Types
// ============================================================================

/// Type of change detected in the USN Journal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    Created,
    Modified,
    Deleted,
    Renamed,
    SecurityChanged,
    PermissionsChanged,
    Other,
}

impl ChangeType {
    /// Convert from USN Journal reason bits
    #[cfg(windows)]
    pub fn from_usn_reason(reason: u32) -> Self {
        const USN_REASON_FILE_CREATE: u32 = 0x0000_0001;
        const USN_REASON_DATA_OVERWRITE: u32 = 0x0000_0002;
        const USN_REASON_DATA_EXTEND: u32 = 0x0000_0004;
        const USN_REASON_DATA_TRUNCATION: u32 = 0x0000_0008;
        const USN_REASON_RENAME_OLD_NAME: u32 = 0x0000_0010;
        const USN_REASON_RENAME_NEW_NAME: u32 = 0x0000_0020;
        const USN_REASON_SECURITY_CHANGE: u32 = 0x0000_0040;

        if reason & USN_REASON_FILE_CREATE != 0 {
            ChangeType::Created
        } else if reason & USN_REASON_RENAME_NEW_NAME != 0 || reason & USN_REASON_RENAME_OLD_NAME != 0 {
            ChangeType::Renamed
        } else if reason & USN_REASON_SECURITY_CHANGE != 0 {
            ChangeType::SecurityChanged
        } else if reason & (USN_REASON_DATA_OVERWRITE | USN_REASON_DATA_EXTEND | USN_REASON_DATA_TRUNCATION) != 0 {
            ChangeType::Modified
        } else {
            ChangeType::Other
        }
    }

    #[cfg(not(windows))]
    pub fn from_usn_reason(_reason: u32) -> Self {
        ChangeType::Other
    }
}

/// A single change record from the USN Journal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsnRecord {
    /// File/directory path that changed
    pub path: PathBuf,

    /// Type of change
    pub change_type: ChangeType,

    /// File reference number (stable identifier)
    pub file_ref: u64,

    /// Parent directory reference number
    pub parent_ref: u64,

    /// Timestamp of the change
    pub timestamp: DateTime<Utc>,

    /// USN value for tracking position in journal
    pub usn: i64,

    /// Whether this is a directory (vs file)
    pub is_directory: bool,
}

// ============================================================================
// USN Journal State Tracking
// ============================================================================

/// Persistent state for tracking position in the USN Journal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct USNJournalState {
    /// Last USN we processed
    pub last_usn: i64,

    /// Journal ID (identifies the journal instance)
    pub journal_id: u64,

    /// Timestamp of last successful read
    pub last_read: DateTime<Utc>,

    /// Drive letter (C, D, etc.)
    pub drive_letter: char,

    /// Count of changes since last sync
    pub change_count: u64,
}

impl Default for USNJournalState {
    fn default() -> Self {
        USNJournalState {
            last_usn: 0,
            journal_id: 0,
            last_read: Utc::now(),
            drive_letter: 'C',
            change_count: 0,
        }
    }
}

// ============================================================================
// USN Journal Tracker
// ============================================================================

/// Tracks changes to a volume via the NTFS USN Journal
pub struct USNTracker {
    root: PathBuf,
    state: USNJournalState,
    buffer: Vec<u8>,
}

impl USNTracker {
    /// Create a new USN tracker for the specified drive
    pub fn new(drive_letter: char, state: USNJournalState) -> Self {
        USNTracker {
            root: PathBuf::from(format!("{}:\\", drive_letter)),
            state,
            buffer: vec![0u8; 65536], // 64KB buffer for USN records
        }
    }

    /// Check if the journal is available and valid
    pub fn is_available(&self) -> DriverResult<bool> {
        #[cfg(windows)]
        {
            Ok(self.get_journal_data().is_ok())
        }
        #[cfg(not(windows))]
        {
            Ok(false)
        }
    }

    /// Get current journal information
    #[cfg(windows)]
    pub fn get_journal_data(&self) -> DriverResult<JournalData> {
        use winapi::um::winioctl::FSCTL_QUERY_USN_JOURNAL;
        use winapi::shared::winerror::ERROR_JOURNAL_NOT_ACTIVE;

        let mut journal_data = unsafe { mem::zeroed::<JournalData>() };
        let mut bytes_returned = 0u32;

        let handle = self.open_volume_handle()?;

        let result = unsafe {
            winapi::um::ioapiset::DeviceIoControl(
                handle,
                FSCTL_QUERY_USN_JOURNAL,
                std::ptr::null_mut(),
                0,
                &mut journal_data as *mut _ as *mut c_void,
                mem::size_of::<JournalData>() as u32,
                &mut bytes_returned,
                std::ptr::null_mut(),
            )
        };

        unsafe { CloseHandle(handle) };

        if result == FALSE {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_JOURNAL_NOT_ACTIVE as i32) {
                return Err(DriverError::JournalNotFound(
                    "USN Journal is not active on this volume".to_string(),
                ));
            }
            return Err(DriverError::Windows(err.to_string()));
        }

        Ok(journal_data)
    }

    #[cfg(not(windows))]
    pub fn get_journal_data(&self) -> DriverResult<JournalData> {
        Err(DriverError::Windows("Not available on non-Windows platforms".to_string()))
    }

    /// Read changes from the journal since last_usn
    pub fn read_changes(&mut self) -> DriverResult<Vec<UsnRecord>> {
        #[cfg(windows)]
        {
            self.read_changes_windows()
        }
        #[cfg(not(windows))]
        {
            Ok(Vec::new())
        }
    }

    /// Windows-specific change reading implementation
    #[cfg(windows)]
    fn read_changes_windows(&mut self) -> DriverResult<Vec<UsnRecord>> {
        use winapi::um::winioctl::FSCTL_READ_USN_JOURNAL;

        let mut read_data = ReadUsnJournalData {
            start_usn: self.state.last_usn,
            reason_mask: 0xFFFFFFFF, // All reasons
            return_only_on_close: FALSE,
            timeout: 0,
            max_versions: 0,
            max_size: self.buffer.len() as u32,
        };

        let mut bytes_returned = 0u32;
        let handle = self.open_volume_handle()?;

        let result = unsafe {
            winapi::um::ioapiset::DeviceIoControl(
                handle,
                FSCTL_READ_USN_JOURNAL,
                &mut read_data as *mut _ as *mut c_void,
                mem::size_of::<ReadUsnJournalData>() as u32,
                self.buffer.as_mut_ptr() as *mut c_void,
                self.buffer.len() as u32,
                &mut bytes_returned,
                std::ptr::null_mut(),
            )
        };

        unsafe { CloseHandle(handle) };

        if result == FALSE {
            return Err(DriverError::Windows(
                std::io::Error::last_os_error().to_string(),
            ));
        }

        // Parse the buffer into USN records
        let buffer_data = self.buffer[..bytes_returned as usize].to_vec();
        self.parse_usn_records(&buffer_data)
    }

    /// Parse USN records from buffer
    fn parse_usn_records(&mut self, buffer: &[u8]) -> DriverResult<Vec<UsnRecord>> {
        let mut records = Vec::new();
        let mut offset = mem::size_of::<i64>(); // Skip the first 8 bytes (next USN)

        while offset < buffer.len() {
            if offset + mem::size_of::<u32>() > buffer.len() {
                break;
            }

            // Read the record length
            let record_len = u32::from_le_bytes([
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
            ]) as usize;

            if record_len == 0 || offset + record_len > buffer.len() {
                break;
            }

            // Parse the record
            if let Ok(record) = self.parse_single_record(&buffer[offset..offset + record_len]) {
                let usn = record.usn;
                records.push(record);
                self.state.last_usn = usn;
                self.state.change_count += 1;
            }

            offset += record_len;
        }

        self.state.last_read = Utc::now();
        Ok(records)
    }

    /// Parse a single USN record
    fn parse_single_record(&self, buffer: &[u8]) -> DriverResult<UsnRecord> {
        if buffer.len() < 98 {
            // Minimum USN_RECORD_V3 size
            return Err(DriverError::Parse("Record too small".to_string()));
        }

        // Parse fixed fields
        let _record_len = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        let _major_version = u16::from_le_bytes([buffer[4], buffer[5]]);
        let _minor_version = u16::from_le_bytes([buffer[6], buffer[7]]);
        let file_ref = u64::from_le_bytes([
            buffer[8], buffer[9], buffer[10], buffer[11], buffer[12], buffer[13],
            buffer[14], buffer[15],
        ]);
        let parent_ref = u64::from_le_bytes([
            buffer[16], buffer[17], buffer[18], buffer[19], buffer[20], buffer[21],
            buffer[22], buffer[23],
        ]);
        let usn = i64::from_le_bytes([
            buffer[24], buffer[25], buffer[26], buffer[27], buffer[28], buffer[29],
            buffer[30], buffer[31],
        ]);

        let timestamp_raw = i64::from_le_bytes([
            buffer[32], buffer[33], buffer[34], buffer[35], buffer[36], buffer[37],
            buffer[38], buffer[39],
        ]);
        let timestamp = Self::filetime_to_datetime(timestamp_raw);

        let reason = u32::from_le_bytes([
            buffer[40], buffer[41], buffer[42], buffer[43],
        ]);
        let _attributes = u32::from_le_bytes([
            buffer[44], buffer[45], buffer[46], buffer[47],
        ]);
        let _file_version_number = u32::from_le_bytes([
            buffer[48], buffer[49], buffer[50], buffer[51],
        ]);
        let _file_strong_integrity = u32::from_le_bytes([
            buffer[52], buffer[53], buffer[54], buffer[55],
        ]);

        let filename_len = u16::from_le_bytes([buffer[56], buffer[57]]) as usize;
        let filename_offset = u16::from_le_bytes([buffer[58], buffer[59]]) as usize;

        let filename = if filename_offset + filename_len <= buffer.len() {
            let bytes = &buffer[filename_offset..filename_offset + filename_len];
            // Interpret bytes as UTF-16 (2 bytes per character)
            let utf16_chars: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            String::from_utf16_lossy(&utf16_chars).to_string()
        } else {
            String::new()
        };

        Ok(UsnRecord {
            path: self.root.join(&filename),
            change_type: ChangeType::from_usn_reason(reason),
            file_ref,
            parent_ref,
            timestamp,
            usn,
            is_directory: _attributes & 0x10 != 0, // FILE_ATTRIBUTE_DIRECTORY
        })
    }

    /// Convert Windows FILETIME to DateTime<Utc>
    fn filetime_to_datetime(filetime: i64) -> DateTime<Utc> {
        const FILETIME_UNIX_DIFF: i64 = 116444736000000000; // 100-nanosecond intervals
        let unix_timestamp = (filetime - FILETIME_UNIX_DIFF) / 10_000_000;

        if unix_timestamp < 0 {
            Utc::now() // Fallback for invalid timestamps
        } else {
            match DateTime::<Utc>::from_timestamp(unix_timestamp, 0) {
                Some(dt) => dt,
                None => Utc::now(),
            }
        }
    }

    /// Check if journal data is still valid
    pub fn check_journal_validity(&mut self) -> DriverResult<bool> {
        #[cfg(windows)]
        {
            let journal_data = self.get_journal_data()?;
            if journal_data.usn_journal_id != self.state.journal_id {
                self.state.last_usn = 0;
                self.state.journal_id = journal_data.usn_journal_id;
                return Ok(false); // Journal was reset
            }
            Ok(true)
        }
        #[cfg(not(windows))]
        {
            Ok(false)
        }
    }

    /// Open a handle to the volume
    #[cfg(windows)]
    fn open_volume_handle(&self) -> DriverResult<*mut c_void> {
        let volume_path = format!("\\\\.\\{}:", self.root.display().to_string().chars().next().unwrap());
        let wide: Vec<u16> = volume_path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return Err(DriverError::InvalidHandle(
                format!("Failed to open volume: {}", self.root.display()),
            ));
        }

        Ok(handle as *mut c_void)
    }

    /// Get the current state (for persistence)
    pub fn state(&self) -> &USNJournalState {
        &self.state
    }

    /// Update the state
    pub fn set_state(&mut self, state: USNJournalState) {
        self.state = state;
    }
}

// ============================================================================
// Windows API Structures (bincode-serializable)
// ============================================================================

/// USN Journal data from FSCTL_QUERY_USN_JOURNAL
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JournalData {
    pub usn_journal_id: u64,
    pub first_usn: i64,
    pub next_usn: i64,
    pub lowest_valid_usn: i64,
    pub max_usn: i64,
    pub max_size: u64,
    pub allocation_size: u64,
}

impl Default for JournalData {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}

/// Read data for FSCTL_READ_USN_JOURNAL
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ReadUsnJournalData {
    pub start_usn: i64,
    pub reason_mask: u32,
    pub return_only_on_close: i32,
    pub timeout: u32,
    pub max_versions: u32,
    pub max_size: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_type_creation() {
        assert_eq!(ChangeType::Created, ChangeType::Created);
    }

    #[test]
    fn test_default_state() {
        let state = USNJournalState::default();
        assert_eq!(state.last_usn, 0);
        assert_eq!(state.drive_letter, 'C');
    }
}
