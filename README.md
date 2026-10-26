# walrus

_WAL + Rust = walrus_

A small key-value store in Rust that keeps its data in a HashMap and uses a
write-ahead log (WAL) so the data is not lost when the program stops.

I made this to learn how a WAL works. It is a learning project, not for real use.

## What it does

- `set(key, value)` and `delete(key)` first write the change to a log file,
  then update the HashMap.
- `get(key)` reads from the HashMap.
- When you open the store again, it reads the log from the start and
  rebuilds the HashMap.
- Each log entry has a CRC checksum, so a damaged entry is detected.
- If the last entry was cut off (like in a crash), it is removed when the
  store opens, so new writes don't go after broken bytes.
- Every write calls `sync_all`, so the entry is on disk before `set` returns.

## Example

```rust
use walrus::kv_store::KvStore;

let mut db = KvStore::open("data.log")?;
db.set(b"name", b"walrus")?;
db.set(b"lang", b"rust")?;
db.delete(b"lang")?;

// close the program, run it again

let db = KvStore::open("data.log")?;
assert_eq!(db.get(b"name"), Some(b"walrus".as_slice()));
assert_eq!(db.get(b"lang"), None);
```

## How the log file looks

Two layers.

**Log entry** (`file.rs`), numbers are big-endian:

```
[crc: 4 bytes][len: 4 bytes][payload: len bytes]
```

The crc is calculated over `len` and `payload`. Entries are written one after
another.

**Operation** (`kv_store.rs`), this goes inside the payload:

```
Set    = [1][key_len: 4 bytes][key][value]
Delete = [2][key]
```

## What happens when opening

1. `scan` reads entries one by one until the end of the file.
2. If the file ends in the middle of an entry, the file is cut back to the end
   of the last good entry.
3. If a full entry has a wrong checksum, `open` returns `CorruptedData`. It does
   not try to fix it.
4. Each good entry is decoded and applied to a new HashMap, in order.

## Run tests

```
cargo test
```

Tests use temp files, one file per test. They cover: round trip of entries,
torn entries, flipped bytes, and data surviving a reopen.

## Not done / known problems

- `set` can write an entry bigger than `MAX_ENTRY_SIZE` (16 MB), and then the
  next `open` fails because `scan` rejects it. It should refuse it in `set`.
- If a write fails halfway, the broken bytes stay in the file and the next
  write goes after them.
- The file is opened and closed on every write. It is slow.
- The log only grows. There is no compaction or snapshot or checkpoints.
- No transactions. Each `set` is its own change.
- Only one process should use a log file. There is no file lock.
- I only tested crashes by cutting bytes off the file, not real power loss.
