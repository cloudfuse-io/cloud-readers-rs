use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::{mpsc::unbounded_channel, Semaphore};

use super::file_manager::{Download, FileCache, FileManager};
use super::{CacheStats, DownloadStat};
use super::{Downloader, FileDescription, Range};

type DownloaderMap = Arc<Mutex<HashMap<String, Arc<dyn Downloader>>>>;

#[derive(Hash, PartialEq, Eq)]
struct CacheKey {
    pub downloader_id: String,
    pub uri: String,
}

type FileCacheMap = Arc<Mutex<HashMap<CacheKey, FileCache>>>;

/// [Start Here] Structure for caching download clients and downloaded data chunks.
///
/// The Download cache converts [`FileDescription`] trait objects into instances of [`FileManager`],
/// registering the downloader and the file URI while doing so.
/// The actual download strategy is specified on the [`FileManager`] object.
pub struct DownloadCache {
    data: FileCacheMap,
    downloaders: DownloaderMap,
    semaphore: Arc<Semaphore>,
    stats: CacheStats,
    release_rate: usize,
    current_max_parallel: Arc<AtomicUsize>,
    absolute_max_parallel: usize,
}

impl DownloadCache {
    /// Create a cache capable of storing data chunks for multiple files.
    /// * `max_parallel` - The maximum number of parallel downloads
    pub fn new(max_parallel: usize) -> Self {
        DownloadCache::new_with(max_parallel, 1, max_parallel)
    }

    /// Same as `new(max_parallel)`, but with finer options:
    /// * `initial_permits` - The maximum number of parallel downloads initially
    /// * `release_rate` - The number of new downloads that can be started each time a download finishes.
    /// A `release_rate` of 1 maintains the maximum parallel downloads to the `initial_permits`.
    /// * `max_parallel` - The maximum number of parallel downloads
    pub fn new_with(initial_permits: usize, release_rate: usize, max_parallel: usize) -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            downloaders: Arc::new(Mutex::new(HashMap::new())),
            semaphore: Arc::new(tokio::sync::Semaphore::new(initial_permits)),
            stats: CacheStats::new(),
            release_rate,
            current_max_parallel: Arc::new(AtomicUsize::new(initial_permits)),
            absolute_max_parallel: max_parallel,
        }
    }

    /// Converts a [`FileDescription`] trait object into a [`FileManager`].
    /// Spawns a task that will download the file chunks queued on the [`FileManager`].
    /// TODO do not re-download chunks if same file was already registered
    pub async fn register(&mut self, file_description: Box<dyn FileDescription>) -> FileManager {
        let register_time = Instant::now();
        let (tx, mut rx) = unbounded_channel::<Range>();
        let file_cache;
        {
            let mut data_guard = self.data.lock().unwrap();
            file_cache = data_guard
                .entry(CacheKey {
                    downloader_id: file_description.get_downloader_id(),
                    uri: file_description.get_uri(),
                })
                .or_insert_with(|| FileCache::new(file_description.get_file_size()))
                .clone();
        }
        let file_manager = FileManager::new(file_cache.clone(), tx);
        let downloader_ref = self.register_downloader(&*file_description);
        let semaphore_ref = Arc::clone(&self.semaphore);
        let stat_ref = self.stats.clone();
        let release_rate = self.release_rate;
        let absolute_max_parallel = self.absolute_max_parallel;
        let current_max_parallel = Arc::clone(&self.current_max_parallel);
        let uri = file_description.get_uri();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                // obtain a permit, it will be released once the download completes
                let permit = semaphore_ref.acquire().await.unwrap();
                permit.forget();
                // run download in a dedicated task
                let downloader_ref = Arc::clone(&downloader_ref);
                let semaphore_ref = Arc::clone(&semaphore_ref);
                let current_max_parallel = Arc::clone(&current_max_parallel);
                let stats_ref = stat_ref.clone();
                let file_cache = file_cache.clone();
                let uri = uri.clone();
                tokio::spawn(async move {
                    // start the actual download
                    let dl_start_time = Instant::now();

                    let dl_res = downloader_ref
                        .download(uri.clone(), message.start, message.length)
                        .await;
                    // once the download is completed, release the permit at the configured rate
                    let new_permits = DownloadCache::permit_leap(
                        &current_max_parallel,
                        absolute_max_parallel,
                        release_rate,
                    );
                    semaphore_ref.add_permits(new_permits);
                    let dl_enum = match dl_res {
                        Ok(downloaded_chunk) => {
                            stats_ref.record_download(DownloadStat {
                                dl_duration: dl_start_time.elapsed().as_millis() as u64,
                                size: downloaded_chunk.len() as u64,
                                dl_start: dl_start_time.duration_since(register_time).as_millis()
                                    as u64,
                            });
                            Download::Done(Arc::new(downloaded_chunk))
                        }
                        Err(err) => Download::Error(format!("{:?}", err)),
                    };
                    file_cache.insert(message.start, dl_enum);
                });
            }
        });
        file_manager
    }

    /// Provides a reference that gives access to the internal statistics in a synchronized fashion.
    pub fn get_stats(&self) -> &CacheStats {
        &self.stats
    }

    fn register_downloader(&self, file_description: &dyn FileDescription) -> Arc<dyn Downloader> {
        let downloader_id = file_description.get_downloader_id();
        let mut dls_guard = self.downloaders.lock().unwrap();
        let current = dls_guard.get(&downloader_id);
        match &current {
            Some(downloader) => Arc::clone(downloader),
            None => {
                let new_downloader = file_description.get_downloader();
                dls_guard.insert(downloader_id, Arc::clone(&new_downloader));
                new_downloader
            }
        }
    }

    fn permit_leap(
        current_max_parallel: &AtomicUsize,
        absolute_max_parallel: usize,
        release_rate: usize,
    ) -> usize {
        let old_current = current_max_parallel
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| {
                if x + release_rate >= absolute_max_parallel {
                    Some(absolute_max_parallel)
                } else {
                    Some(x + release_rate)
                }
            })
            .unwrap();
        std::cmp::min(
            release_rate,
            std::cmp::max(absolute_max_parallel - old_current, 1),
        )
    }
}

impl fmt::Debug for DownloadCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data_guard = self.data.lock().unwrap();
        for (key, file_cache) in &*data_guard {
            write!(
                f,
                "file = {} / {}:\n{:?}",
                key.downloader_id, key.uri, file_cache
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_permit_leap() {
        // follow release rate if you can
        assert_eq!(DownloadCache::permit_leap(&AtomicUsize::new(1), 3, 1), 1);
        assert_eq!(DownloadCache::permit_leap(&AtomicUsize::new(1), 3, 2), 2);
        // release what remains before reaching max otherwise
        assert_eq!(DownloadCache::permit_leap(&AtomicUsize::new(1), 3, 3), 2);
        assert_eq!(DownloadCache::permit_leap(&AtomicUsize::new(1), 2, 3), 1);
        // permit leap must be at least one
        assert_eq!(DownloadCache::permit_leap(&AtomicUsize::new(3), 3, 3), 1);
    }
}
