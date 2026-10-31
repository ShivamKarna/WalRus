use std::fmt;
use std::ops::Bound;
use std::{collections::BTreeMap, fs::OpenOptions, path::Path};

use crate::{
    errors::FileError,
    file::{LogEntry, Tail, append_entry, scan},
};

// Two layers, two formats.

// Layer 1: log record (file.rs 'LogEntry')
// it just frames the raw bytes
//      [crc: u32][len : u32][payload : len bytes]  big-endian

// Layer 2 : operation (this file, 'Op')

#[derive(PartialEq, Eq)]
pub enum Op {
    Set { key: Vec<u8>, val: Vec<u8> },
    Delete { key: Vec<u8> },
}

impl fmt::Debug for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::Set { key, val } => write!(
                f,
                "Set({} => {})",
                String::from_utf8_lossy(key),
                String::from_utf8_lossy(val)
            ),
            Op::Delete { key } => write!(f, "Delete({})", String::from_utf8_lossy(key)),
        }
    }
}

impl Op {
    // Set = [1][key_len: u32 BE][key][value]
    // Delete = [2][key]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Op::Set { key, val } => {
                let mut buffer = vec![1u8];
                buffer.extend_from_slice(&(key.len() as u32).to_be_bytes());
                buffer.extend_from_slice(key);
                buffer.extend_from_slice(val);
                buffer
            }
            Op::Delete { key } => {
                let mut buffer = vec![2u8];
                buffer.extend_from_slice(key);
                buffer
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Op, FileError> {
        let (tag, rest) = bytes.split_first().ok_or(FileError::CorruptedData)?;

        match tag {
            1 => {
                if rest.len() < 4 {
                    return Err(FileError::CorruptedData);
                }
                let key_len = u32::from_be_bytes(rest[0..4].try_into().unwrap()) as usize;
                let rest = &rest[4..];
                if rest.len() < key_len {
                    return Err(FileError::CorruptedData);
                }
                let (key, val) = rest.split_at(key_len);
                Ok(Op::Set {
                    key: key.to_vec(),
                    val: val.to_vec(),
                })
            }
            2 => Ok(Op::Delete { key: rest.to_vec() }),
            _ => Err(FileError::CorruptedData),
        }
    }
}

fn apply(map: &mut BTreeMap<Vec<u8>, Vec<u8>>, op: Op) {
    match op {
        Op::Set { key, val } => {
            map.insert(key, val);
        }
        Op::Delete { key } => {
            map.remove(&key);
        }
    }
}

pub struct KvStore {
    path: String,
    map: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl KvStore {
    pub fn open(path: &str) -> Result<KvStore, FileError> {
        let mut map = BTreeMap::new();

        if Path::new(path).exists() {
            let result = scan(path)?;

            // cutting of the last torn entries
            if result.tail == Tail::Torn {
                let file = OpenOptions::new().write(true).open(path)?;
                file.set_len(result.valid_len)?;
                file.sync_all()?;
            }

            // Replay: REDO every logged change, in order.
            for entry in result.entries {
                let op = Op::decode(entry.payload())?;
                apply(&mut map, op);
            }
        }
        Ok(KvStore {
            path: path.to_string(),
            map,
        })
    }

    pub fn set(&mut self, key: &[u8], val: &[u8]) -> Result<(), FileError> {
        let op = Op::Set {
            key: key.to_vec(),
            val: val.to_vec(),
        };

        append_entry(&self.path, &LogEntry::new(op.encode()))?; // log first
        apply(&mut self.map, op); // then apply in memory
        Ok(())
    }

    pub fn delete(&mut self, key: &[u8]) -> Result<(), FileError> {
        let op = Op::Delete { key: key.to_vec() };

        append_entry(&self.path, &LogEntry::new(op.encode()))?;
        apply(&mut self.map, op);
        Ok(())
    }

    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.map.get(key).map(|v| v.as_slice())
    }

    pub fn scan_range(&self, start: &[u8], end: &[u8]) -> Vec<(&[u8], &[u8])> {
        if start > end {
            return Vec::new();
        }

        self.map
            .range::<[u8], _>((Bound::Included(start), Bound::Excluded(end)))
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect()
    }
}

#[cfg(test)]
mod test {
    use crate::file::MAX_ENTRY_SIZE;

    use super::*;
    use std::fs::{self};

    fn temp_path(name: &str) -> String {
        let mut p = std::env::temp_dir();
        p.push(format!("walrus_store_{}_{}", std::process::id(), name));
        let _ = fs::remove_file(&p);
        p.to_str().unwrap().to_string()
    }

    #[test]
    fn values_survive_reopen() {
        let path = temp_path("survive.log");
        {
            let mut store = KvStore::open(&path).unwrap();
            store.set(b"name", b"walrus").unwrap();
            store.set(b"lang", b"rust").unwrap();
        } // store dropped here : the in-memory map is gone
        let store = KvStore::open(&path).unwrap(); // rebuiltt from log
        assert_eq!(store.get(b"name"), Some(b"walrus".as_slice()));
        assert_eq!(store.get(b"lang"), Some(b"rust".as_slice()));

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn delete_survives_reopen() {
        let path = temp_path("delete.log");
        {
            let mut store = KvStore::open(&path).unwrap();
            store.set(b"a", b"1").unwrap();
            store.delete(b"a").unwrap();
        }

        let store = KvStore::open(&path).unwrap();
        assert_eq!(store.get(b"a"), None);

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn torn_tail_is_dropped_and_store_keeps_working() {
        let path = temp_path("torn.log");

        {
            let mut store = KvStore::open(&path).unwrap();
            store.set(b"a", b"1").unwrap();
            store.set(b"b", b"2").unwrap();
            store.set(b"c", b"3").unwrap();
        }

        // simulate a crash:  cut 3 bytes off the last entry
        let len = fs::metadata(&path).unwrap().len();
        let f = OpenOptions::new().write(true).open(&path).unwrap();

        f.set_len(len - 3).unwrap();

        {
            let mut store = KvStore::open(&path).unwrap();
            assert_eq!(store.get(b"a"), Some(b"1".as_slice()));
            assert_eq!(store.get(b"b"), Some(b"2".as_slice()));
            assert_eq!(store.get(b"c"), None); // torn entry was never written

            store.set(b"d", b"4").unwrap(); // this must not land after garbage
        }

        let store = KvStore::open(&path).unwrap();

        assert_eq!(store.get(b"d"), Some(b"4".as_slice()));
        assert_eq!(store.get(b"c"), None);

        fs::remove_file(&path).unwrap();
    }

    // Op test
    #[test]
    fn op_roundtrip() {
        let ops = vec![
            Op::Set {
                key: b"name".to_vec(),
                val: b"walrus".to_vec(),
            },
            Op::Set {
                key: b"a".to_vec(),
                val: vec![],
            }, // empty value
            Op::Set {
                key: vec![],
                val: b"v".to_vec(),
            }, // empty key
            Op::Delete {
                key: b"name".to_vec(),
            },
        ];

        for op in ops {
            assert_eq!(Op::decode(&op.encode()).unwrap(), op);
        }
    }

    #[test]
    fn op_decode_rejects_bad_input() {
        //empty input
        assert!(matches!(Op::decode(&[]), Err(FileError::CorruptedData)));
        // unknown header
        assert!(matches!(
            Op::decode(&[9, 1, 2]),
            Err(FileError::CorruptedData)
        ));
        // set with fewer than 4 length bytes
        assert!(matches!(
            Op::decode(&[1, 0, 0]),
            Err(FileError::CorruptedData)
        ));
    }
    // KvStore Behaviour tests

    #[test]
    fn open_missing_file_gives_empty_store() {
        let path = temp_path("open_missing.log");
        let store = KvStore::open(&path).unwrap();
        assert_eq!(store.get(b"anything"), None);
    }

    #[test]
    fn open_empty_file_gives_empty_store() {
        let path = temp_path("empty_store.log");
        fs::write(&path, b"").unwrap();

        let store = KvStore::open(&path).unwrap();
        assert_eq!(store.get(b"anything"), None);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn delete_missing_key_is_ok() {
        let p = temp_path("delete_missing.log");

        let mut s = KvStore::open(&p).unwrap();
        s.delete(b"never_existed").unwrap();
        assert_eq!(s.get(b"never_existed"), None);

        fs::remove_file(&p).unwrap();
    }

    #[test]
    fn overwrite_keeps_latest_value() {
        let path = temp_path("overwrite_keeps_latest.log");

        {
            let mut s = KvStore::open(&path).unwrap();
            s.set(b"wow", b"wow value").unwrap();
            s.set(b"wow", b"wow value changed").unwrap();
            assert_eq!(s.get(b"wow"), Some(b"wow value changed".as_slice()));
        }

        let s = KvStore::open(&path).unwrap();

        assert_eq!(s.get(b"wow"), Some(b"wow value changed".as_slice()));

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn empty_value_is_not_same_as_missing() {
        let path = temp_path("empty_value_is_not_missing.log");

        {
            let mut s = KvStore::open(&path).unwrap();
            s.set(b"wow", b"").unwrap();
        }

        let s = KvStore::open(&path).unwrap();
        assert_eq!(s.get(b"wow"), Some(b"".as_slice())); // Some(empty) not 'None'

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn torn_update_brings_back_old_value() {
        let path = temp_path("torn_update.log");

        {
            let mut s = KvStore::open(&path).unwrap();
            s.set(b"key1", b"val1").unwrap();
            s.set(b"key1", b"val2").unwrap();
        }

        let len = fs::metadata(&path).unwrap().len();
        let file = OpenOptions::new().write(true).open(&path).unwrap();

        file.set_len(len - 3).unwrap();
        let s = KvStore::open(&path).unwrap();

        assert_eq!(s.get(b"key1"), Some(b"val1".as_slice()));

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn damage_in_the_middle_is_corrupted_data() {
        let path = temp_path("damaged_in_middle.log");

        {
            let mut s = KvStore::open(&path).unwrap();
            s.set(b"key1", b"val1").unwrap();
            s.set(b"key2", b"val2").unwrap();
            s.set(b"key3", b"val3").unwrap();
        }

        let mut bytes = fs::read(&path).unwrap();
        bytes[8] ^= 0xFF;

        fs::write(&path, bytes).unwrap();

        assert!(matches!(
            KvStore::open(&path),
            Err(FileError::CorruptedData)
        ));

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn oversized_value_is_rejected_at_write_time() {
        let path = temp_path("oversized_value.log");

        let mut s = KvStore::open(&path).unwrap();

        s.set(b"k", b"ok").unwrap();

        let huge = vec![0u8; MAX_ENTRY_SIZE + 1];
        assert!(matches!(s.set(b"c", &huge), Err(FileError::EntryTooLarge)));

        drop(s);

        let s = KvStore::open(&path).unwrap();
        assert_eq!(s.get(b"c"), None);
        assert_eq!(s.get(b"k"), Some(b"ok".as_slice()));

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn largest_allowed_entry_roundtrip() {
        let path = temp_path("largest.log");
        let key = b"k";

        // payload = tag 1 + key_len 4(BE) + key + value , it must be exactly MAX_ENTRY_SIZE
        let val = vec![7u8; MAX_ENTRY_SIZE - 1 - 4 - key.len()];

        {
            let mut st = KvStore::open(&path).unwrap();
            st.set(key, &val).unwrap();
        }

        let s = KvStore::open(&path).unwrap();
        assert_eq!(s.get(key), Some(val.as_slice()));

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn scan_range_returns_half_open_range() {
        let path = temp_path("scan_range.log");
        let mut s = KvStore::open(&path).unwrap();

        for k in [b"a", b"b", b"c", b"d"] {
            s.set(k, b"v").unwrap();
        }

        let got: Vec<&[u8]> = s
            .scan_range(b"b".as_slice(), b"d".as_slice())
            .into_iter()
            .map(|(k, _)| k)
            .collect();

        assert_eq!(got, vec![b"b".as_slice(), b"c".as_slice()]);

        assert!(s.scan_range(b"d", b"b").is_empty()); // start > end

        assert!(s.scan_range(b"b", b"b").is_empty()); // start == end

        drop(s);

        fs::remove_file(&path).unwrap();
    }
}
