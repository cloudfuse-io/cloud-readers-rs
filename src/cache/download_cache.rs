use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::unbounded_channel;

use super::{Download, Downloader, FileCache, FileHandle, Range};

type DownloaderMap = Arc<Mutex<HashMap<String, Arc<dyn Downloader>>>>;

#[derive(Hash, PartialEq, Eq)]
struct CacheKey {
    pub downloader_id: String,
    pub uri: String,
}

type FileCacheMap = Arc<Mutex<HashMap<CacheKey, FileCache>>>;

/// [Start Here] Structure for caching download clients and downloaded data chunks.
///
/// The Download cache converts [`FileHandle`] trait objects into instances of [`FileCache`],
/// registering the downloader and the file URI while doing so.
/// The actual download strategy is specified on the [`FileCache`] object.
pub struct DownloadCache {
    data: FileCacheMap,
    downloaders: DownloaderMap,
}

impl DownloadCache {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            downloaders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Converts a [`FileHandle`] trait object into a [`FileCache`].
    /// Spawns a task that will download the file chunks queued on the [`FileCache`].
    /// TODO do not re-download chunks if same file was already registered
    pub async fn register(&mut self, file_handle: Box<dyn FileHandle>) -> FileCache {
        let (tx, mut rx) = unbounded_channel::<Range>();
        let file_cache;
        {
            let mut data_guard = self.data.lock().unwrap();
            file_cache = data_guard
                .entry(CacheKey {
                    downloader_id: file_handle.get_downloader_id(),
                    uri: file_handle.get_uri(),
                })
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
            write!(
                f,
                "file = {} / {}:\n{:?}",
                key.downloader_id, key.uri, file_cache
            )?;
        }
        Ok(())
    }
}
