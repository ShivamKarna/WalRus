# walrus

A small key-value store in Rust. Data lives in a `HashMap`. A write-ahead log (WAL) on disk lets it survive restarts and crashes.

A learning project, not for production use.

## Contents

- [Usage](#usage)
  - [Library](#library)
  - [CLI](#cli)
- [Design](#design)
  - [Write path](#write-path)
  - [Recovery](#recovery)
  - [Log format](#log-format)
  - [Layers](#layers)
- [Guarantees and limits](#guarantees-and-limits)
- [Tests](#tests)

## Usage

### Library

```rust
use walrus::kv_store::KvStore;

let mut db = KvStore::open("data.log")?;
db.set(b"name", b"walrus")?;
assert_eq!(db.get(b"name"), Some(b"walrus".as_slice()));
db.delete(b"name")?;
```

Keys and values are arbitrary bytes. `open` creates the log if it does not exist.

### CLI

```
cargo run -- set <key> <value>
cargo run -- get <key>
cargo run -- delete <key>
cargo run -- dump            # print every log entry, repairs nothing
cargo run -- chop <n>        # cut n bytes off the log (simulate a crash)
cargo run -- flip <pos>      # flip one byte (simulate damage)
```

```
$ cargo run -- set name walrus
$ cargo run -- set lang rust
$ cargo run -- delete lang
$ cargo run -- dump
0: Set(name => walrus)
1: Set(lang => rust)
2: Delete(lang)
entries: 3, valid bytes: 57, file bytes: 57, tail: Clean
```

## Design

The map in memory is the working copy. The log on disk is the source of truth. The map can always be rebuilt from the log, never the other way round.

### Write path

`set` and `delete` do two things, in a fixed order:

1. Append the operation to the log and call `sync_all`.
2. Apply the operation to the map.

If step 1 fails, the function returns early and the map is untouched, so memory never holds a change the log doesn't. If the process dies between the two steps, the change is in the log and is applied on the next `open`. Reads (`get`) only touch the map.

Both normal writes and recovery go through the same `apply` function, so the two paths can't behave differently.

### Recovery

`open` reads the log from the start with `scan`, which stops at one of three outcomes:

| Outcome                                                         | Meaning              | What `open` does                                                       |
| --------------------------------------------------------------- | -------------------- | ---------------------------------------------------------------------- |
| End of file at an entry boundary                                | clean log            | replays all entries                                                    |
| End of file in the middle of an entry                           | crash during a write | truncates the file to the end of the last complete entry, then replays |
| A complete entry fails its checksum or has an impossible length | damage               | returns `CorruptedData`, changes nothing                               |

Truncating a cut-off entry is what makes later writes safe. Without it, new entries would land after the broken bytes and never be read back.

An entry that was cut off was never acknowledged, because `set` only returns `Ok` after the sync. Dropping it breaks no promise.

### Log format

Big-endian. The CRC covers `len` and `payload`, so a damaged length is detected too.

```
entry   = [crc: u32][len: u32][payload: len bytes]
payload = Set    [1][key_len: u32][key][value]
        | Delete [2][key]
```

`len` is capped at 16 MB. The cap is checked before any allocation, so a corrupted length can't trigger a huge allocation, and it is also enforced on write, so the log can never contain an entry it refuses to read.

### Layers

- `file.rs`: entries only. It frames bytes, checksums them, appends them, and scans them back. It knows nothing about what the payload means.
- `kv_store.rs`: defines the operations (`Op`), encodes them into payloads, and owns the map.

## Guarantees and limits

- Once `set` or `delete` returns `Ok`, the entry has been synced to disk.
- If the log ends in a partial entry (a crash during a write), `open` truncates it. Everything before it is kept.
- If a complete entry fails its checksum, `open` returns `CorruptedData`. It does not skip or repair.
- Entries larger than 16 MB are rejected on write.
- One process per log file. There is no locking.
- The log only grows. There is no compaction.
- Crash behavior is tested by truncating and corrupting the file, not by power-loss testing.

## Tests

```
cargo test
```
