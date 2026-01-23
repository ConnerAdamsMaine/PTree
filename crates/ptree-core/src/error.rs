use thiserror::Error;
use std::io;

#[derive(Error, Debug)]
pub enum PTreeError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    
    #[error("Cache error: {0}")]
    Cache(String),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    
    #[error("Invalid drive: {0}")]
    InvalidDrive(String),
    
    #[error("Lock timeout: {0}")]
    LockTimeout(String),
    
    #[error("Traversal error: {0}")]
    Traversal(String),
}

pub type PTreeResult<T> = Result<T, PTreeError>;
