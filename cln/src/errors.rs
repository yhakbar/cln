use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("Failed to create tempdir: {0}")]
    TempDirError(std::io::Error),
    #[error("Failed to close tempdir: {0}")]
    TempDirCloseError(std::io::Error),
    #[error("Failed to spawn git command: {0}")]
    CommandSpawnError(std::io::Error),
    #[error("Failed to complete git clone: {0}")]
    GitCloneError(String),
    #[error("Failed to parse git command output: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
    #[error("Failed to create cln-store directory: {0}")]
    CreateDirError(std::io::Error),
    #[error("Failed to find home directory")]
    HomeDirError,
    #[error("No matching reference found")]
    NoMatchingReferenceError,
    #[error("Failed to write {0} to cln-store: {1}")]
    WriteToStoreError(String, std::io::Error),
    #[error("Failed to create directory: {0}")]
    CreateDirAllError(std::io::Error),
    #[error("Failed to hard link: {0}")]
    HardLinkError(std::io::Error),
    #[error("Failed to read tree: {0}")]
    ReadTreeError(std::io::Error),
    #[error("Parse mode error: {0}")]
    ParseModeError(std::num::ParseIntError),
    #[error("Failed to read file {0}: {1}")]
    ReadFileError(String, std::io::Error),
}
