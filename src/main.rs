use std::env;
use std::error::Error;
use std::fs::{self, OpenOptions};

use walrus::file::scan;
use walrus::kv_store::{KvStore, Op};

const LOG: &str = "data.log";

const USAGE: &str = "\
usage:
  set <key> <value>
  get <key>
  delete <key>
  dump          show every entry in the log (does not repair anything)
  chop <n>      cut n bytes off the end of the log (fake a crash)
  flip <pos>    flip the byte at position pos (fake damage)
";

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    match args.as_slice() {
        ["set", k, v] => {
            let mut db = KvStore::open(LOG)?;
            db.set(k.as_bytes(), v.as_bytes())?;
            println!("ok");
        }
        ["get", k] => {
            let db = KvStore::open(LOG)?;
            match db.get(k.as_bytes()) {
                Some(v) => println!("{}", String::from_utf8_lossy(v)),
                None => println!("(not found)"),
            }
        }
        ["delete", k] => {
            let mut db = KvStore::open(LOG)?;
            db.delete(k.as_bytes())?;
            println!("ok");
        }
        ["dump"] => {
            let result = scan(LOG)?;
            for (i, entry) in result.entries.iter().enumerate() {
                println!("{i}: {:?}", Op::decode(entry.payload())?);
            }
            println!(
                "entries: {}, valid bytes: {}, file bytes: {}, tail: {:?}",
                result.entries.len(),
                result.valid_len,
                fs::metadata(LOG)?.len(),
                result.tail
            );
        }
        ["chop", n] => {
            let n: u64 = n.parse()?;
            let len = fs::metadata(LOG)?.len();
            let new_len = len.checked_sub(n).ok_or("file is shorter than that")?;
            OpenOptions::new().write(true).open(LOG)?.set_len(new_len)?;
            println!("file: {len} -> {new_len} bytes");
        }
        ["flip", pos] => {
            let pos: usize = pos.parse()?;
            let mut bytes = fs::read(LOG)?;
            if pos >= bytes.len() {
                return Err(format!("file has only {} bytes", bytes.len()).into());
            }
            bytes[pos] ^= 0xFF;
            fs::write(LOG, &bytes)?;
            println!("flipped byte {pos}");
        }
        _ => print!("{USAGE}"),
    }
    Ok(())
}
