use thiserror::Error;
use std::io;

#[derive(Error, Debug)]
pub enum DriverError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Windows API error: {0}")]
    Windows(String),

    #[error("USN Journal error: {0}")]
    UsnJournal(String),

    #[error("Invalid handle: {0}")]
    InvalidHandle(String),

    #[error("Buffer too small: {0}")]
    BufferTooSmall(String),

    #[error("Journal not found: {0}")]
    JournalNotFound(String),

    #[error("Access denied: {0}")]
    AccessDenied(String),

    #[error("Parse error: {0}")]
    Parse(String),
}

pub type DriverResult<T> = Result<T, DriverError>;
