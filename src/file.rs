use crate::errors::FileError;
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::sync::Arc;
use std::thread;

pub fn read_from_file(input_path: &str) -> Result<Vec<u8>, FileError> {
    let bytes = fs::read(input_path)?;

    if bytes.is_empty() {
        return Err(FileError::EmptyFile);
    }
    Ok(bytes)
}

pub fn write_to_file(file_path: &str, content: &[u8]) -> Result<(), FileError> {
    let result = OpenOptions::new()
        .write(true)
        .create(true)
        .append(true)
        .open(file_path);

    let mut file = match result {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            return Err(FileError::FileAlreadyExists(file_path.to_string()));
        }
        Err(e) => {
            return Err(FileError::Io(e));
        }
    };

    file.write_all(content)?;
    file.sync_all()?;
    Ok(())
}
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn file_is_being_read() {
        let result = read_from_file("./test.txt").unwrap();
        assert_eq!(result, b"wow this is working");
    }
    #[test]
    fn content_is_being_written_in_file() {
        let file_path = "./walrus.txt";
        let content = [1, 2, 3];

        write_to_file(file_path, &content).unwrap();

        let result = read_from_file(file_path).unwrap();
        assert_eq!(result, content);

        fs::remove_file(file_path).expect("Failed to delete test file.");
    }
}
