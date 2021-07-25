use std::io::{self, Read, Seek, SeekFrom};

use anyhow::anyhow;

use super::FileCache;

/// Cursor that allows the to Read/Seek through a [`FileCache`]
///
/// Blocks if bytes are read that were not yet downloaded
/// Fails if bytes are read that were not scheduled for downloading
#[derive(Clone)]
pub struct FileCacheCursor {
    pub cache: FileCache,
    pub position: u64,
}

impl Read for FileCacheCursor {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.position == self.cache.get_file_size() {
            return Ok(0);
        }
        let cache_cursor = self
            .cache
            .get(self.position)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let bytes_read = cache_cursor.read(buf);

        // update reader position
        self.position += bytes_read as u64;
        Ok(bytes_read)
    }
}

impl Seek for FileCacheCursor {
    /// implementation inspired from std::io::Cursor
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        let (base_pos, offset) = match pos {
            SeekFrom::Start(n) => {
                self.position = n;
                return Ok(n);
            }
            SeekFrom::End(n) => (self.cache.get_file_size(), n),
            SeekFrom::Current(n) => (self.position, n),
        };
        let new_pos = if offset >= 0 {
            base_pos.checked_add(offset as u64)
        } else {
            base_pos.checked_sub((offset.wrapping_neg()) as u64)
        };
        match new_pos {
            Some(n) => {
                self.position = n;
                Ok(self.position)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                anyhow!("invalid seek to a negative or overflowing position"),
            )),
        }
    }
}
