use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::{mpsc::unbounded_channel, Semaphore};

use super::file_manager::{Download, FileCache, FileManager};
use super::stats::CacheStats;
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
    stats: Arc<CacheStats>,
}

impl DownloadCache {
    /// Create a cache capable of storing data chunks for multiple files.
    /// * `max_parallel` - The maximum number of parallel downloads
    pub fn new(max_parallel: usize) -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            downloaders: Arc::new(Mutex::new(HashMap::new())),
            semaphore: Arc::new(tokio::sync::Semaphore::new(max_parallel)),
            stats: Arc::new(CacheStats::new()),
        }
    }

    /// Converts a [`FileDescription`] trait object into a [`FileManager`].
    /// Spawns a task that will download the file chunks queued on the [`FileManager`].
    /// TODO do not re-download chunks if same file was already registered
    pub async fn register(&mut self, file_description: Box<dyn FileDescription>) -> FileManager {
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
        let stat_ref = Arc::clone(&self.stats);
        let uri = file_description.get_uri();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                // obtain a permit, it will be released once the download completes
                let start_time = Instant::now();
                let permit = semaphore_ref.acquire().await.unwrap();
                permit.forget();
                stat_ref.inc_waiting_download_ms(start_time.elapsed().as_millis() as u64);
                // run download in a dedicated task
                let downloader_ref = Arc::clone(&downloader_ref);
                let semaphore_ref = Arc::clone(&semaphore_ref);
                let stats_ref = Arc::clone(&stat_ref);
                let file_cache = file_cache.clone();
                let uri = uri.clone();
                tokio::spawn(async move {
                    // start the actual download
                    let dl_res = downloader_ref
                        .download(uri.clone(), message.start, message.length)
                        .await;
                    // once the download is completed, release the permit
                    semaphore_ref.add_permits(1);
                    let dl_enum = match dl_res {
                        Ok(downloaded_chunk) => {
                            stats_ref.inc_downloaded_bytes(downloaded_chunk.len() as u64);
                            stats_ref.inc_download_count(1);
                            Download::Done(Arc::new(downloaded_chunk))
                        }
                        Err(err) => Download::Error(format!("{}", err)),
                    };
                    file_cache.insert(message.start, dl_enum);
                });
            }
        });
        file_manager
    }

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
