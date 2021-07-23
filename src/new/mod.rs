use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};

use anyhow::{anyhow, bail, ensure, Result};
use async_trait::async_trait;
use futures::stream::Stream;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

type Uri = String;

#[async_trait]
pub trait Downloader: Send + Sync {
    async fn download(&self, uri: Uri, start: u64, length: usize) -> Result<Vec<u8>>;
}

pub struct Range {
    pub start: u64,
    pub length: usize,
}

pub trait RangeStream: Stream<Item = Range> + Send + Sync + Unpin {}

impl<T: Stream<Item = Range> + Send + Sync + Unpin> RangeStream for T {}

pub trait FileHandle: Send {
    fn get_downloader(&self) -> Arc<dyn Downloader>;

    fn get_downloader_id(&self) -> String;

    fn get_uri(&self) -> Uri;

    fn get_file_size(&self) -> u64;
}

/// The status and content of the download
#[derive(Clone)]
pub enum Download {
    Pending,
    Done(Arc<Vec<u8>>),
    Error(String),
}

#[derive(Clone)]
pub struct FileCache {
    ranges: Arc<Mutex<BTreeMap<u64, Download>>>,
    cv: Arc<Condvar>,
    file_size: u64,
    tx: UnboundedSender<Range>,
}

type DownloaderMap = Arc<Mutex<HashMap<String, Arc<dyn Downloader>>>>;
type CacheKey = (String, Uri);
type FileCacheMap = Arc<Mutex<HashMap<CacheKey, FileCache>>>;

pub struct DownloadCache {
    data: FileCacheMap,
    downloaders: DownloaderMap,
}

/// A single useage cursor that helps reading from a cached data chunk
pub struct RangeCursor {
    data: Arc<Vec<u8>>,
    offset: u64,
}

impl RangeCursor {
    /// Construct new RangeCursor while ensuring that the offset is within the expected bounds
    pub fn try_new(data: Arc<Vec<u8>>, offset: u64) -> Result<Self> {
        ensure!(
            data.len() > offset as usize,
            "Out of bound in RangeCursor: (offset={}) >= (length={})",
            offset,
            data.len(),
        );
        Ok(Self { data, offset })
    }
    /// Consumes self as RangeCursor is read only once
    pub fn read(self, buf: &mut [u8]) -> usize {
        // compute len to read
        let len = std::cmp::min(buf.len(), self.data.len() - self.offset as usize);
        // get downloaded data
        buf[0..len]
            .clone_from_slice(&self.data[self.offset as usize..(self.offset as usize + len)]);
        len
    }
}

/// A cursor that allows the to Read/Seek through a FileCache
/// Blocks if bytes are read that were not yet downloaded
/// Fails if bytes are read that were not scheduled for downloading
struct FileCacheCursor {
    pub cache: FileCache,
    pub position: u64,
}

impl Read for FileCacheCursor {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
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

impl DownloadCache {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            downloaders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // TODO should this be async or not???
    // TODO do not re-download chunks if same file was already registered
    pub async fn register(&mut self, file_handle: Box<dyn FileHandle>) -> FileCache {
        let (tx, mut rx) = unbounded_channel::<Range>();
        let file_cache;
        {
            let mut data_guard = self.data.lock().unwrap();
            file_cache = data_guard
                .entry((file_handle.get_downloader_id(), file_handle.get_uri()))
                .or_insert_with(|| FileCache::new(file_handle.get_file_size(), tx))
                .clone();
        }
        let file_cache_res = file_cache.clone();
        let downloader_ref = self.register_downloader(&*file_handle);
        tokio::spawn(async move {
            let uri = file_handle.get_uri();
            while let Some(message) = rx.recv().await {
                let downloader_ref = Arc::clone(&downloader_ref);
                let file_cache = file_cache.clone();
                let uri = uri.clone();
                // run download in a dedicated task
                tokio::spawn(async move {
                    let dl_res = downloader_ref
                        .download(uri.clone(), message.start, message.length)
                        .await;
                    let dl_enum = match dl_res {
                        Ok(downloaded_chunk) => Download::Done(Arc::new(downloaded_chunk)),
                        Err(err) => Download::Error(format!("{}", err)),
                    };
                    file_cache.insert(message.start, dl_enum);
                });
            }
        });
        file_cache_res
    }

    fn register_downloader(&self, file_handle: &dyn FileHandle) -> Arc<dyn Downloader> {
        let downloader_id = file_handle.get_downloader_id();
        let mut dls_guard = self.downloaders.lock().unwrap();
        let current = dls_guard.get(&downloader_id);
        match &current {
            Some(downloader) => Arc::clone(downloader),
            None => {
                let new_downloader = file_handle.get_downloader();
                dls_guard.insert(downloader_id, Arc::clone(&new_downloader));
                new_downloader
            }
        }
    }
}

impl fmt::Debug for DownloadCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data_guard = self.data.lock().unwrap();
        for (key, file_cache) in &*data_guard {
            write!(f, "{:?}", key)?;
            let range_guard = file_cache.ranges.lock().unwrap();
            for (pos, dl) in &*range_guard {
                write!(f, "-- {}", pos)?;
            }
        }
        Ok(())
    }
}

impl FileCache {
    pub fn new(file_size: u64, tx: UnboundedSender<Range>) -> Self {
        Self {
            ranges: Arc::new(Mutex::new(BTreeMap::new())),
            cv: Arc::new(std::sync::Condvar::new()),
            file_size,
            tx,
        }
    }

    pub fn queue_download(&mut self, ranges: Vec<Range>) -> Result<()> {
        for range in ranges {
            self.insert(range.start, Download::Pending);
            self.tx.send(range).map_err(|e| anyhow!(e.to_string()))?;
        }
        Ok(())
    }

    pub fn get_file_size(&self) -> u64 {
        self.file_size
    }

    // Insert download result and notifies blocked readers that something changes
    pub fn insert(&self, start: u64, download: Download) {
        let mut range_guard = self.ranges.lock().unwrap();
        range_guard.insert(start, download);
        self.cv.notify_all()
    }

    /// Get a chunk from the cache
    /// For now the cache can only get get single chunck readers and fails if the dl was not scheduled
    /// If the download is not finished, this waits synchronously for the chunk to be ready
    pub fn get(&self, start: u64) -> Result<RangeCursor> {
        use std::ops::Bound::{Included, Unbounded};
        let mut ranges_guard = self.ranges.lock().unwrap();

        let mut before = ranges_guard
            .range((Unbounded, Included(start)))
            .next_back()
            .map(|(start, dl)| (*start, dl.clone()));

        while let Some((_, Download::Pending)) = before {
            // wait for the dl to be finished
            ranges_guard = self.cv.wait(ranges_guard).unwrap();
            before = ranges_guard
                .range((Unbounded, Included(start)))
                .next_back()
                .map(|(start, dl)| (*start, dl.clone()));
        }

        let before = before.ok_or(anyhow!("Download not scheduled at position {})", start))?;

        let unused_start = start - before.0;

        match before.1 {
            Download::Done(bytes) => RangeCursor::try_new(bytes, unused_start),
            Download::Error(err) => bail!(err.to_owned()),
            Download::Pending => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_correct_ranges() {
        let mut download_cache = DownloadCache::new();

        let mock_file_handle = MockFileHandle::new(1000);

        let mut file_cache = download_cache.register(Box::new(mock_file_handle)).await;

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
        let mut buf = vec![0u8, 50];
        let read_result = cursor.read(&mut buf).expect("Read from file cache failed");
        assert_eq!(read_result, 50usize);
        println!("{:?}", download_cache);

        // TODO
        // - check that the pattern is correct (use read_from_cache from old test fixtures)
    }

    #[tokio::test]
    async fn test_correct_ranges_spawn() {
        let mut download_cache = DownloadCache::new();

        let mock_file_handle = MockFileHandle::new(1000);

        let mut file_cache = download_cache.register(Box::new(mock_file_handle)).await;

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

        let (buf, read_result) = tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8, 50];
            let read_result = cursor.read(&mut buf).expect("Read from file cache failed");
            (buf, read_result)
        })
        .await
        .unwrap();

        assert_eq!(read_result, 50usize);
        println!("{:?}", download_cache);

        // TODO
        // - check that the pattern is correct (use read_from_cache from old test fixtures)
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
        async fn download(&self, _file: String, _start: u64, length: usize) -> Result<Vec<u8>> {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(pattern(0, length))
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

        fn get_uri(&self) -> Uri {
            "mock_uri".to_owned()
        }

        fn get_file_size(&self) -> u64 {
            self.length
        }
    }
}
