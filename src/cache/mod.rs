use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex};

use anyhow::{anyhow, bail, ensure, Result};
use itertools::Itertools;
use tokio::sync::mpsc::UnboundedSender;

mod cursor;
mod download_cache;
mod models;

pub use cursor::FileCacheCursor;
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

/// Container of the references stored by the download cache for a given file.
///
/// To get a FileCache, you need to register a [`FileHandle`] on a [`DownloadCache`].
/// You can then schedule the download of the file chunks using [`queue_download`](Self::queue_download).
#[derive(Clone)]
pub struct FileCache {
    ranges: Arc<Mutex<BTreeMap<u64, Download>>>,
    cv: Arc<Condvar>,
    file_size: u64,
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

impl FileCache {
    fn new(file_size: u64, tx: UnboundedSender<Range>) -> Self {
        Self {
            ranges: Arc::new(Mutex::new(BTreeMap::new())),
            cv: Arc::new(std::sync::Condvar::new()),
            file_size,
            tx,
        }
    }

    pub fn queue_download(&mut self, ranges: Vec<Range>) -> Result<()> {
        for range in ranges {
            self.insert(range.start, Download::Pending(range.length));
            self.tx.send(range).map_err(|e| anyhow!(e.to_string()))?;
        }
        Ok(())
    }

    pub fn get_file_size(&self) -> u64 {
        self.file_size
    }

    // Insert download result and notifies blocked readers that something changes
    fn insert(&self, start: u64, download: Download) {
        let mut range_guard = self.ranges.lock().unwrap();
        range_guard.insert(start, download);
        self.cv.notify_all()
    }

    /// Get a chunk from the cache
    /// For now the cache can only get get single chunck readers and fails if the dl was not scheduled
    /// If the download is not finished, this waits synchronously for the chunk to be ready
    fn get(&self, start: u64) -> Result<RangeCursor> {
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

        let before = before.ok_or(anyhow!(
            "Download not scheduled at position {}, scheduled ranges are:\n{}",
            start,
            fmt_debug(&*ranges_guard),
        ))?;

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
            Download::Error(err) => bail!(err.to_owned()),
            Download::Pending(_) => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::io::{self, Read, Seek, SeekFrom};
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_read_within_range() {
        let (download_cache, mut file_cache) = init_mock(1000).await;

        file_cache
            .queue_download(vec![Range {
                start: 0,
                length: 100,
            }])
            .expect("Could not queue Range on handle");

        let mut cursor = FileCacheCursor {
            cache: file_cache,
            position: 0,
        };

        assert_cursor(&cursor, pattern(0, 50))
            .await
            .expect("could not read enough bytes [0:50[ in download [0:100[");
        cursor
            .seek(SeekFrom::Start(20))
            .expect("Cursor could not be moved");
        assert_cursor(&cursor, pattern(20, 70))
            .await
            .expect("Could not read bytes bytes [20:70[ in download [0:100[");

        // check the debug output for the cache
        assert_eq!(
            format!("{:?}", download_cache),
            "\
file = mock_downloader / mock_uri:
-- Start=0000000000 Status=Done(100 bytes)"
        );
    }

    #[tokio::test]
    async fn test_read_accross_ranges() {
        let (download_cache, mut file_cache) = init_mock(1000).await;

        file_cache
            .queue_download(vec![
                Range {
                    start: 200,
                    length: 100,
                },
                Range {
                    start: 0,
                    length: 100,
                },
                Range {
                    start: 100,
                    length: 100,
                },
            ])
            .expect("Could not queue Range on handle");

        let cursor = FileCacheCursor {
            cache: file_cache,
            position: 0,
        };

        assert_cursor(&cursor, pattern(0, 300))
            .await
            .expect("Could not read bytes bytes [0:300[ in download [0:100[+[100:200[+[200:300[");

        // check the debug output for the cache
        assert_eq!(
            format!("{:?}", download_cache),
            "\
file = mock_downloader / mock_uri:
-- Start=0000000000 Status=Done(100 bytes)
-- Start=0000000100 Status=Done(100 bytes)
-- Start=0000000200 Status=Done(100 bytes)"
        );
    }

    #[tokio::test]
    async fn test_read_complete_file() {
        let file_size = 100;
        let (download_cache, mut file_cache) = init_mock(file_size).await;

        // we schedule the download of the whole file
        file_cache
            .queue_download(vec![Range {
                start: 0,
                length: file_size as usize,
            }])
            .expect("Could not queue Range on handle");

        let cursor = FileCacheCursor {
            cache: file_cache,
            position: 0,
        };

        let target = pattern(0, file_size as usize);
        let mut cursor = cursor.clone();
        // perform blocking read in separate thread!
        let result = tokio::task::spawn_blocking(move || -> io::Result<Vec<u8>> {
            let mut content = vec![];
            // we try to read the whole file
            cursor.read_to_end(&mut content)?;
            Ok(content)
        })
        .await
        .unwrap()
        .expect(&format!(
            "Could not read bytes bytes [0:{len}[ in download [0:{len}[",
            len = file_size,
        ));
        assert_eq!(result, target);

        // check the debug output for the cache
        assert_eq!(
            format!("{:?}", download_cache),
            "\
file = mock_downloader / mock_uri:
-- Start=0000000000 Status=Done(100 bytes)"
        );
    }

    #[tokio::test]
    async fn test_read_uninit() {
        let (_, mut file_cache) = init_mock(1000).await;

        let mut cursor = FileCacheCursor {
            cache: file_cache.clone(),
            position: 0,
        };

        // try reading whithout starting any download
        let err_msg = assert_cursor(&cursor, pattern(0, 50))
            .await
            .expect_err("Read should fail if no download was scheduled")
            .to_string();
        assert_eq!(err_msg, "No download scheduled");

        // try reading before of downloaded range
        file_cache
            .queue_download(vec![Range {
                start: 200,
                length: 100,
            }])
            .expect("Could not queue Range on handle");
        let err_msg = assert_cursor(&cursor, pattern(0, 50))
            .await
            .expect_err("Read of [0:50[ should fail, only [200,300[ was downloaded")
            .to_string();
        assert_eq!(
            err_msg,
            "\
Download not scheduled at position 0, scheduled ranges are:
-- Start=0000000200 Status=Pending(100 bytes)"
        );

        // try reading after of downloaded range
        cursor
            .seek(SeekFrom::Start(280))
            .expect("Cursor could not be moved");
        let err_msg = assert_cursor(&cursor, pattern(0, 50))
            .await
            .expect_err("Read of [280:320[ should fail, only [200,300[ was downloaded")
            .to_string();
        assert_eq!(
            err_msg,
            "\
Download not scheduled at position 300, scheduled ranges are:
-- Start=0000000200 Status=Done(100 bytes)"
        );
    }

    //// Test Fixtures: ////

    /// A downloader that returns a simple pattern (1,2,3...254,255,1,2...)
    /// Waits for 10ms before returning its result to trigger cache misses
    struct MockDownloader;

    /// The pattern (1,2,3...254,255,1,2...) in the range [start,end[
    fn pattern(start: usize, end: usize) -> Vec<u8> {
        (start..end).map(|i| (i % 256) as u8).collect::<Vec<_>>()
    }

    #[async_trait]
    impl Downloader for MockDownloader {
        async fn download(&self, _file: String, start: u64, length: usize) -> Result<Vec<u8>> {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(pattern(start as usize, start as usize + length))
        }
    }

    struct MockFileHandle {
        length: u64,
    }

    impl MockFileHandle {
        fn new(length: u64) -> Self {
            Self { length }
        }
    }

    impl FileHandle for MockFileHandle {
        fn get_downloader(&self) -> Arc<dyn Downloader> {
            Arc::new(MockDownloader)
        }

        fn get_downloader_id(&self) -> String {
            "mock_downloader".to_owned()
        }

        fn get_uri(&self) -> String {
            "mock_uri".to_owned()
        }

        fn get_file_size(&self) -> u64 {
            self.length
        }
    }

    async fn init_mock(len: u64) -> (DownloadCache, FileCache) {
        let mut download_cache = DownloadCache::new();

        let mock_file_handle = MockFileHandle::new(len);

        let file_cache = download_cache.register(Box::new(mock_file_handle)).await;

        (download_cache, file_cache)
    }

    /// Assert that the next `target.len()` bytes of `cursor` match the bytes in `target`
    /// SPAWNS A NEW THREAD TO PERFORM THE READ BECAUSE IT IS BLOCKING!
    async fn assert_cursor(cursor: &FileCacheCursor, target: Vec<u8>) -> io::Result<()> {
        let target_length = target.len();
        let mut cursor = cursor.clone();
        // perform blocking read in separate thread!
        let result = tokio::task::spawn_blocking(move || -> io::Result<Vec<u8>> {
            let mut content = vec![0u8; target_length];
            cursor.read_exact(&mut content)?;
            Ok(content)
        })
        .await
        .unwrap()?;
        assert_eq!(result, target);
        Ok(())
    }
}
