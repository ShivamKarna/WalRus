// Custom error handling, and Error Types.

use core::fmt;
use std::error::Error;
use std::io;
use std::string::FromUtf8Error;

#[derive(Debug)]
pub enum FileError {
    Io(io::Error),
    EmptyFile,
    CorruptedData,
    InvalidUtf8,
    FileAlreadyExists(String),
    ThreadPanic(String),
    EntryTooLarge,
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::Io(e) => write!(f, "I/O error : {}", e),
            FileError::CorruptedData => {
                write!(f, "failed to open file , file is corrputed ")
            }
            FileError::EmptyFile => {
                write!(f, "file is empty, nothing to read")
            }
            FileError::InvalidUtf8 => {
                write!(f, "file is not a valid UTF-8 text")
            }
            FileError::FileAlreadyExists(path) => {
                write!(
                    f,
                    "output file '{}' already exists, refusing to overwrite",
                    path
                )
            }
            FileError::ThreadPanic(msg) => {
                write!(
                    f,
                    "a worker thread panicked while working with file {}",
                    msg
                )
            }
            FileError::EntryTooLarge => write!(f, "entry is larger than the maximum allowed size"),
        }
    }
}

impl Error for FileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            FileError::Io(e) => Some(e),
            _ => None,
        }
    }
}

// Lets `?` convert io::Error automatically
impl From<io::Error> for FileError {
    fn from(err: io::Error) -> Self {
        FileError::Io(err)
    }
}

impl From<FromUtf8Error> for FileError {
    fn from(_err: FromUtf8Error) -> Self {
        FileError::InvalidUtf8
    }
}
