use thiserror::Error;

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum TxtError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid byte offset {offset} in rope of length {len}")]
    InvalidOffset { offset: usize, len: usize },
}

#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, anyhow::Error>;
