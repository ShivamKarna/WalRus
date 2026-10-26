use crate::errors::FileError;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::sync::Arc;
use std::thread;

#[cfg(unix)]
use std::os::unix::fs::FileExt;

#[cfg(windows)]
use std::os::windows::fs::FileExt;

// Constants
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
