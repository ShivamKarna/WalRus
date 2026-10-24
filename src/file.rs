use crate::errors::FileError;
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::sync::Arc;
use std::thread;

#[cfg(unix)]
use std::os::unix::fs::FileExt;

#[cfg(windows)]
use std::os::windows::fs::FileExt;

const MAX_ENTRY_SIZE: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct LogEntry {
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

        if payload_len > MAX_ENTRY_SIZE {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "entry length too large",
            ));
        }
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

pub fn append_to_file(file_path: &str, content: &[u8]) -> Result<(), FileError> {
    // let mut file = match OpenOptions::new().create(true).append(true).open(file_path) {
    //     Ok(f) => f,
    //     Err(e) => {
    //         return Err(FileError::Io(e));
    //     }
    // };

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;

    file.write_all(content)?;
    file.sync_all()?;
    Ok(())
}

pub fn create_new_file(file_path: &str, content: &[u8]) -> Result<(), FileError> {
    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(file_path)
    {
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

// core function
pub fn append_entry(path: &str, entry: &LogEntry) -> Result<(), FileError> {
    let data_to_be_appended = entry.serialize();
    append_to_file(path, &data_to_be_appended)
}
#[cfg(test)]
mod test {
    use super::*;

    // helpers
    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();

        p.push(format!("walrus_{}_{}", std::process::id(), name));
        let _ = fs::remove_file(&p); // ignore error if it doesn't exist.
        p
    }

    fn entry(s: &str) -> LogEntry {
        LogEntry::new(s.as_bytes().to_vec())
    }

    #[test]
    fn file_is_being_read() {
        let p = temp_path("read.txt");
        let path = p.to_str().unwrap();
        fs::write(path, b"wow this is working").unwrap();

        let result = read_from_file(path).unwrap();
        assert_eq!(result, b"wow this is working");

        fs::remove_file(path).unwrap();
    }
    #[test]
    fn create_new_file_writes_content_to_file() {
        let p = temp_path("create_new.txt");
        let path = p.to_str().unwrap();

        create_new_file(path, &[1, 2, 3]).unwrap();

        assert_eq!(read_from_file(path).unwrap(), [1, 2, 3]);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn creat_new_file_refuses_second_write() {
        let p = temp_path("crate_new_exists.txt");
        let path = p.to_str().unwrap();

        let _ = create_new_file(path, &[1]);
        let second = create_new_file(path, &[2]);
        assert!(matches!(second, Err(FileError::FileAlreadyExists(_))));
        assert_eq!(read_from_file(path).unwrap(), [1]);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn log_entry_serialization_roundtrip() {
        let original_entry = entry("WOW THIS IS THE WAL ENTRY");

        let mut reader = io::Cursor::new(original_entry.serialize());
        let decoded_entry = LogEntry::deserialize(&mut reader).unwrap();
        assert_eq!(original_entry, decoded_entry);
    }

    #[test]
    fn deserialize_rejects_huge_file() {
        let mut reader = io::Cursor::new(vec![0xFF, 0xFF, 0xFF, 0xFF]);
        let err = LogEntry::deserialize(&mut reader).unwrap_err();

        assert_eq!(err.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn append_entry_appends_in_order() {
        let p = temp_path("read.txt");
        let path = p.to_str().unwrap();

        let entry1 = LogEntry::new(Vec::from("Hello World From Entry 1"));
        let entry2 = LogEntry::new(Vec::from("This is Entry 2"));

        append_entry(path, &entry1).unwrap();
        append_entry(path, &entry2).unwrap();

        let mut expected = Vec::<u8>::new();
        expected.extend_from_slice(&entry1.serialize());
        expected.extend_from_slice(&entry2.serialize());

        assert_eq!(read_from_file(path).unwrap(), expected);

        fs::remove_file(path).unwrap();
    }
}
