use std::{collections::HashMap, fs::OpenOptions, path::Path};

use crate::{
    errors::FileError,
    file::{LogEntry, Tail, append_entry, scan},
};

// Two layers, two formats.

// Layer 1: log record (file.rs 'LogEntry')
// it just frames the raw bytes
//      [crc: u23][len : u32][payload : len bytes]  big-endian

// Layer 2 : operation (this file, 'Op')
pub enum Op {
    Set { key: Vec<u8>, val: Vec<u8> },
    Delete { key: Vec<u8> },
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

fn apply(map: &mut HashMap<Vec<u8>, Vec<u8>>, op: Op) {
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
    map: HashMap<Vec<u8>, Vec<u8>>,
}

impl KvStore {
    pub fn open(path: &str) -> Result<KvStore, FileError> {
        let mut map = HashMap::new();

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
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fs;

    // unit tests for above
}
