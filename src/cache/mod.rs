use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex};

use anyhow::{anyhow, bail, ensure, Result};
use itertools::Itertools;
use tokio::sync::mpsc::UnboundedSender;

mod cursor;
mod download_cache;
mod mock;
mod models;

pub use cursor::CacheCursor;
pub use download_cache::DownloadCache;
pub use models::*;

/// The status and content of the download
#[derive(Clone)]
enum Download {
    Pending(usize),
    Done(Arc<Vec<u8>>),
    Error(String),
}

impl fmt::Debug for Download {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Download::Pending(len) => write!(f, "Pending({} bytes)", len),
            Download::Done(data) => write!(f, "Done({} bytes)", data.len()),
            Download::Error(error) => write!(f, "Error({:?})", error),
        }
    }
}

/// A single useage cursor that helps reading from a cached data chunk
struct RangeCursor {
    data: Arc<Vec<u8>>,
    offset: u64,
}

impl RangeCursor {
    /// Construct new RangeCursor while ensuring that the offset is within the expected bounds
    fn try_new(data: Arc<Vec<u8>>, offset: u64) -> Result<Self> {
        ensure!(
            data.len() > offset as usize,
            "Out of bound in RangeCursor: (offset={}) >= (length={})",
            offset,
            data.len(),
        );
        Ok(Self { data, offset })
    }
    /// Consumes self as RangeCursor is read only once
    fn read(self, buf: &mut [u8]) -> usize {
        // compute len to read
        let len = std::cmp::min(buf.len(), self.data.len() - self.offset as usize);
        // get downloaded data
        buf[0..len]
            .clone_from_slice(&self.data[self.offset as usize..(self.offset as usize + len)]);
        len
    }
}

#[derive(Clone)]
struct FileCache {
    ranges: Arc<Mutex<BTreeMap<u64, Download>>>,
    cv: Arc<Condvar>,
    file_size: u64,
}

impl FileCache {
    fn new(file_size: u64) -> Self {
        Self {
            ranges: Arc::new(Mutex::new(BTreeMap::new())),
            cv: Arc::new(Condvar::new()),
            file_size,
        }
    }

    /// Insert download result and notifies blocked readers that something changes
    fn insert(&self, start: u64, download: Download) {
        let mut range_guard = self.ranges.lock().unwrap();
        range_guard.insert(start, download);
        self.cv.notify_all()
    }

    fn get_range(&self, start: u64) -> Result<RangeCursor> {
        use std::ops::Bound::{Included, Unbounded};
        let mut ranges_guard = self.ranges.lock().unwrap();

        ensure!(ranges_guard.len() > 0, "No download scheduled");

        let mut before = ranges_guard
            .range((Unbounded, Included(start)))
            .next_back()
            .map(|(start, dl)| (*start, dl.clone()));

        while let Some((_, Download::Pending(_))) = before {
            // wait for the dl to be finished
            ranges_guard = self.cv.wait(ranges_guard).unwrap();
            before = ranges_guard
                .range((Unbounded, Included(start)))
                .next_back()
                .map(|(start, dl)| (*start, dl.clone()));
        }

        let before = before.ok_or_else(|| {
            anyhow!(
                "Download not scheduled at position {}, scheduled ranges are:\n{}",
                start,
                fmt_debug(&*ranges_guard),
            )
        })?;

        match before.1 {
            Download::Done(bytes) => {
                ensure!(
                    before.0 + bytes.len() as u64 > start,
                    "Download not scheduled at position {}, scheduled ranges are:\n{}",
                    start,
                    fmt_debug(&*ranges_guard),
                );
                RangeCursor::try_new(bytes, start - before.0)
            }
            Download::Error(err) => bail!(err),
            Download::Pending(_) => unreachable!(),
        }
    }
}

/// Container of the references stored by the download cache for a given file.
///
/// To get a [`FileManager`], you need to register a [`FileDescription`] on a [`DownloadCache`].
/// You can then schedule the download of the file chunks using [`queue_download`](Self::queue_download).
#[derive(Clone)]
pub struct FileManager {
    cache: FileCache,
    tx: UnboundedSender<Range>,
}

fn fmt_debug(map: &BTreeMap<u64, Download>) -> String {
    map.iter()
        .map(|(pos, dl)| format!("-- Start={:0>10} Status={:?}", pos, dl))
        .join("\n")
}

impl fmt::Debug for FileCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let range_guard = self.ranges.lock().unwrap();
        write!(f, "{}", fmt_debug(&*range_guard))
    }
}

impl FileManager {
    fn new(cache: FileCache, tx: UnboundedSender<Range>) -> Self {
        Self { cache, tx }
    }

    /// Schedule new downloads for the file
    /// The downloads will be started in the specified order.
    pub fn queue_download(&self, ranges: Vec<Range>) -> Result<()> {
        for range in ranges {
            self.cache
                .insert(range.start, Download::Pending(range.length));
            self.tx.send(range).map_err(|e| anyhow!(e.to_string()))?;
        }
        Ok(())
    }

    /// Get a chunk from the cache
    /// For now the cache can only get single chunck readers and fails if the dl was not scheduled
    /// If the download is not finished, this waits synchronously for the chunk to be ready
    fn get_range(&self, start: u64) -> Result<RangeCursor> {
        self.cache.get_range(start)
    }

    pub fn get_file_size(&self) -> u64 {
        self.cache.file_size
    }
}

#[cfg(test)]
mod tests {
    use super::mock::*;
    use super::*;

    #[tokio::test]
    async fn test_read_at_0() {
        let file_manager = init_mock(1000).await;

        file_manager
            .queue_download(vec![Range {
                start: 0,
                length: 100,
            }])
            .expect("Could not queue Range on handle");

        assert_cursor(file_manager, 0, pattern(0, 100))
            .await
            .expect("Could not get range at 0 with download of [0:100[");
    }

    #[tokio::test]
    async fn test_read_with_offset() {
        let file_manager = init_mock(1000).await;

        file_manager
            .queue_download(vec![Range {
                start: 0,
                length: 100,
            }])
            .expect("Could not queue Range on handle");

        assert_cursor(file_manager, 50, pattern(50, 100))
            .await
            .expect("Could not get range at 50 with download of [0:100[");
    }

    #[tokio::test]
    async fn test_read_uninited() {
        let file_manager = init_mock(1000).await;
        let err_msg = assert_cursor(file_manager, 0, pattern(0, 100))
            .await
            .expect_err("Read file without queued downloads should fail")
            .to_string();

        assert_eq!(err_msg, "No download scheduled");
    }

    #[tokio::test]
    async fn test_read_outside_download() {
        let file_manager = init_mock(1000).await;

        file_manager
            .queue_download(vec![Range {
                start: 0,
                length: 100,
            }])
            .expect("Could not queue Range on handle");

        let err_msg = assert_cursor(file_manager, 120, pattern(120, 200))
            .await
            .expect_err("Read file without scheduled downloads should fail")
            .to_string();

        assert_eq!(
            err_msg,
            "\
Download not scheduled at position 120, scheduled ranges are:
-- Start=0000000000 Status=Done(100 bytes)"
        );
    }

    async fn init_mock(len: u64) -> FileManager {
        let mut download_cache = DownloadCache::new(1);

        let mock_file_description = MockFileDescription::new(len);

        download_cache
            .register(Box::new(mock_file_description))
            .await
    }

    /// Assert that the first `target.len()` bytes of `cursor` match the bytes in `target`
    /// SPAWNS A NEW THREAD TO GET THE RANGE BECAUSE IT IS BLOCKING!
    async fn assert_cursor(file_manager: FileManager, start: u64, target: Vec<u8>) -> Result<()> {
        let target_length = target.len();
        // perform blocking get in separate thread!
        let cursor = tokio::task::spawn_blocking(move || -> Result<RangeCursor> {
            file_manager.get_range(start)
        })
        .await
        .unwrap()?;
        // verify the bytes in the range
        let mut content = vec![0u8; target_length];
        cursor.read(&mut content);
        assert_eq!(content, target);
        Ok(())
    }
}
