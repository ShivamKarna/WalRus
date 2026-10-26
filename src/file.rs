// HEADER : [crc][length]
// Format : HEADER + payload

use crate::errors::FileError;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::sync::Arc;
use std::thread;

#[cfg(unix)]
use std::os::unix::fs::FileExt;

#[cfg(windows)]
use std::os::windows::fs::FileExt;

// Constants
const MAX_ENTRY_SIZE: usize = 16 * 1024 * 1024;
const HEADER_LEN: usize = 8; // crc (4 bytes) + len(4 bytes) ,

#[derive(Debug, PartialEq, Eq)]
pub struct LogEntry {
    payload: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Tail {
    Clean,
    Torn,
}

#[derive(Debug)]
pub struct ScanResult {
    pub entries: Vec<LogEntry>,
    pub valid_len: u64,
    pub tail: Tail,
}

pub fn scan(path: &str) -> Result<ScanResult, FileError> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut entries = Vec::<LogEntry>::new();
    let mut valid_len: u64 = 0;

    let tail = loop {
        if reader.fill_buf()?.is_empty() {
            break Tail::Clean;
        }

        match LogEntry::deserialize(&mut reader) {
            Ok(entry) => {
                valid_len += entry.encoded_len() as u64;
                entries.push(entry)
            }
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => break Tail::Torn,
            Err(e) if e.kind() == ErrorKind::InvalidData => {
                return Err(FileError::CorruptedData);
            }
            Err(e) => return Err(FileError::Io(e)),
        }
    };

    Ok(ScanResult {
        entries,
        valid_len,
        tail,
    })
}

impl LogEntry {
    pub fn new(payload: Vec<u8>) -> Self {
        Self { payload }
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn encoded_len(&self) -> usize {
        HEADER_LEN + self.payload.len()
    }

    pub fn serialize(&self) -> Vec<u8> {
        // length
        // hasher builder and finalize crc
        // take an empty buffer and extend it with crc, and len, and payload
        let len_bytes = (self.payload.len() as u32).to_be_bytes();

        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&len_bytes);
        hasher.update(&self.payload);
        let crc = hasher.finalize();

        let mut buffer = Vec::with_capacity(self.encoded_len());
        buffer.extend_from_slice(&crc.to_be_bytes());
        buffer.extend_from_slice(&len_bytes);
        buffer.extend_from_slice(&self.payload);
        buffer
    }

    pub fn deserialize<R: Read>(reader: &mut R) -> io::Result<Self> {
        // deserialization order : [crc][len][payload]

        // crc bytes
        let mut crc_bytes = [0u8; 4];
        reader.read_exact(&mut crc_bytes)?;
        let stored_crc = u32::from_be_bytes(crc_bytes);

        // len bytes
        let mut len_bytes = [0u8; 4];
        reader.read_exact(&mut len_bytes)?;
        let payload_len = u32::from_be_bytes(len_bytes) as usize;

        if payload_len > MAX_ENTRY_SIZE {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "entry length too large!!",
            ));
        }

        // payload bytes
        let mut payload = vec![0u8; payload_len];
        reader.read_exact(&mut payload)?;

        // re compute hash to match checksum, crc
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&len_bytes);
        hasher.update(&payload);

        if hasher.finalize() != stored_crc {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "checksum mismatched",
            ));
        }

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

pub fn append_to_file(path: &str, content: &[u8]) -> Result<(), FileError> {
    // let mut file = match OpenOptions::new().create(true).append(true).open(file_path) {
    //     Ok(f) => f,
    //     Err(e) => {
    //         return Err(FileError::Io(e));
    //     }
    // };

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;

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

// core functions
pub fn append_entry(path: &str, entry: &LogEntry) -> Result<(), FileError> {
    let serialized_entry = entry.serialize();
    append_to_file(path, &serialized_entry)
}

pub fn read_entries(path: &str) -> Result<Vec<LogEntry>, FileError> {
    Ok(scan(path)?.entries)
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

    // cuts 'n' bytes from end of file, trying to simulate crash mid-write
    fn chop_tail(path: &str, n: u64) {
        let len = fs::metadata(path).unwrap().len();

        let file = OpenOptions::new().write(true).open(path).unwrap();
        file.set_len(len - n).unwrap();
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
    fn create_new_file_refuses_second_write() {
        let p = temp_path("create_new_exists.txt");
        let path = p.to_str().unwrap();

        create_new_file(path, &[1]).unwrap();
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
        let mut reader = io::Cursor::new(vec![0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
        let err = LogEntry::deserialize(&mut reader).unwrap_err();

        assert_eq!(err.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn append_entry_appends_in_order() {
        let p = temp_path("append_order.txt");
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

    #[test]
    fn read_entries_reads_entries_in_order() {
        let p = temp_path("replay.log");
        let path = p.to_str().unwrap();

        for s in ["one", "two", "three"] {
            append_entry(path, &entry(s)).unwrap();
        }

        assert_eq!(
            read_entries(path).unwrap(),
            vec![entry("one"), entry("two"), entry("three")]
        );

        fs::remove_file(path).unwrap();
    }
    #[test]
    fn torn_payload_is_ignored() {
        let p = temp_path("torn_payload.log");
        let path = p.to_str().unwrap();

        for s in ["one", "two", "three"] {
            append_entry(path, &entry(s)).unwrap();
        }

        // cut 3 bytes of last entry
        chop_tail(path, 3);

        assert_eq!(
            read_entries(path).unwrap(),
            vec![entry("one"), entry("two")]
        );

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn torn_header_is_ignored() {
        let p = temp_path("torn_header.log");
        let path = p.to_str().unwrap();

        for s in ["one", "two", "three"] {
            append_entry(path, &entry(s)).unwrap();
        }

        // cut 7 bytes of last entry
        // three is 9 bytes, 4 header + 5 bytes, so cutting of 7 bytes leaves us with torn header
        chop_tail(path, 7);

        assert_eq!(
            read_entries(path).unwrap(),
            vec![entry("one"), entry("two")]
        );

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn garbage_length_is_reported_as_corrupted_data() {
        let p = temp_path("garbage_length.log");
        let path = p.to_str().unwrap();

        for s in ["one", "two", "three"] {
            append_entry(path, &entry(s)).unwrap();
        }

        append_entry(path, &entry("entry")).unwrap();
        append_to_file(path, &[0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]).unwrap();

        assert!(matches!(read_entries(path), Err(FileError::CorruptedData)));

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn scan_clean_log() {
        let p = temp_path("scan_clean");
        let path = p.to_str().unwrap();

        for s in ["one", "two", "three"] {
            append_entry(path, &entry(s)).unwrap();
        }

        let scan_result = scan(path).unwrap();

        let total: usize = ["one", "two", "three"]
            .iter()
            .map(|s| entry(s).encoded_len())
            .sum();

        assert_eq!(
            scan_result.entries,
            vec![entry("one"), entry("two"), entry("three")]
        );
        assert_eq!(scan_result.valid_len, total as u64);
        assert_eq!(scan_result.tail, Tail::Clean);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn scan_torn_payload() {
        let p = temp_path("scan_torn.log");
        let path = p.to_str().unwrap();

        for s in ["one", "two", "three"] {
            append_entry(path, &entry(s)).unwrap();
        }

        chop_tail(path, 3);

        let scan_result = scan(path).unwrap();

        let good_len = entry("one").encoded_len() + entry("two").encoded_len();

        assert_eq!(scan_result.entries, vec![entry("one"), entry("two")]);
        assert_eq!(scan_result.valid_len, good_len as u64);
        assert_eq!(scan_result.tail, Tail::Torn);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn scan_torn_header() {
        let p = temp_path("scan_torn_header.log");
        let path = p.to_str().unwrap();

        for s in ["one", "two", "three"] {
            append_entry(path, &entry(s)).unwrap();
        }

        chop_tail(path, entry("three").encoded_len() as u64 - 2);

        let scan_result = scan(path).unwrap();

        let good_len = entry("one").encoded_len() + entry("two").encoded_len();

        assert_eq!(scan_result.entries.len(), 2);
        assert_eq!(scan_result.valid_len, good_len as u64);
        assert_eq!(scan_result.tail, Tail::Torn);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn flipped_payload_byte_read_as_corrupted_data() {
        let p = temp_path("flipped_payload.log");
        let path = p.to_str().unwrap();

        append_entry(path, &entry("hello")).unwrap();
        let mut bytes = fs::read(path).unwrap();

        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;

        fs::write(path, &bytes).unwrap();

        assert!(matches!(scan(path), Err(FileError::CorruptedData)));

        fs::remove_file(path).unwrap();
    }
}
