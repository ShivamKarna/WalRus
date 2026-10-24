use crate::errors::FileError;
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::sync::Arc;
use std::thread;

#[cfg(unix)]
use std::os::unix::fs::FileExt;

#[cfg(windows)]
use std::os::windows::fs::FileExt;

#[derive(Debug, PartialEq, Eq)]
struct LogEntry {
    payload: Vec<u8>,
}

impl LogEntry {
    pub fn new(payload: Vec<u8>) -> Self {
        Self { payload }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let payload_len = self.payload.len() as u32;
        let mut buffer = Vec::with_capacity(4 + self.payload.len());

        buffer.extend_from_slice(&payload_len.to_be_bytes());
        buffer.extend_from_slice(&self.payload);

        buffer
    }

    pub fn deserialize<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut len_bytes = [0u8; 4];

        reader.read_exact(&mut len_bytes)?;

        let payload_len = u32::from_be_bytes(len_bytes) as usize;
        let mut payload = vec![0u8; payload_len];

        reader.read_exact(&mut payload)?;

        Ok(LogEntry { payload })
    }
}

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
// This might be nonsense for now, but maybe useful later, i guess while backup or huge huge file writing. IDK....
pub fn parallel_thread_writer(input_path: &str, data: &[u8]) -> Result<(), FileError> {
    if data.is_empty() {
        return Ok(());
    }

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(input_path)?;

    // Pre allocating the file size , so that the chunk size or offset doesn't grows dynamically.
    file.set_len(data.len() as u64)?;

    const TARGET_CHUNK_SIZE: usize = 1024 * 1024; // 1 MB per chunk

    let available_threads = thread::available_parallelism().map_or(4, |n| n.get());

    let thread_count = available_threads
        .min(data.len().div_ceil(TARGET_CHUNK_SIZE))
        .max(1);

    // sequential(normal) write operation for small files.
    if thread_count <= 1 || data.len() < TARGET_CHUNK_SIZE {
        let mut f = file;
        f.write_all(data)?;
        f.sync_all()?;
        return Ok(());
    }

    let chunk_size = data.len().div_ceil(thread_count);
    let file_arc = Arc::new(file);
    let mut handles = vec![];

    for (i, chunk) in data.chunks(chunk_size).enumerate() {
        let offset = (i * chunk_size) as u64;
        let chunk_data = chunk.to_vec();
        let file_clone = Arc::clone(&file_arc);

        let handle = thread::spawn(move || -> Result<(), io::Error> {
            #[cfg(unix)]
            {
                file_clone.write_all_at(&chunk_data, offset)?;
            }
            #[cfg(windows)]
            {
                file_clone.seek_write(&chunk_data, offset)?;
            }
            // Ok::<(), io::Error>(())
            Ok(())
        });
        handles.push(handle);
    }

    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(FileError::Io(e)),
            Err(payload) => {
                let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic!".to_string()
                };
                return Err(FileError::ThreadPanic(msg));
            }
        }
    }
    file_arc.sync_all()?;

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

    #[test]
    fn log_entry_serialization_roundtrip() {
        let original_entry = LogEntry::new(b"WAL Entry in file".to_vec());

        let serialized_entry = original_entry.serialize();
        let mut reader = std::io::Cursor::new(serialized_entry);
        let decoded_entry = LogEntry::deserialize(&mut reader).unwrap();
        assert_eq!(original_entry, decoded_entry);
    }
}
