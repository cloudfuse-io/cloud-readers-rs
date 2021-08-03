use std::sync::atomic::{AtomicU64, Ordering};

pub struct CacheStats {
    downloaded_bytes: AtomicU64,
    waiting_download_ms: AtomicU64,
    download_count: AtomicU64,
}

impl CacheStats {
    pub(crate) fn new() -> Self {
        Self {
            downloaded_bytes: AtomicU64::new(0),
            waiting_download_ms: AtomicU64::new(0),
            download_count: AtomicU64::new(0),
        }
    }

    pub fn downloaded_bytes(&self) -> u64 {
        self.downloaded_bytes.load(Ordering::Relaxed)
    }
    pub fn waiting_download_ms(&self) -> u64 {
        self.waiting_download_ms.load(Ordering::Relaxed)
    }
    pub fn download_count(&self) -> u64 {
        self.download_count.load(Ordering::Relaxed)
    }

    pub(crate) fn inc_downloaded_bytes(&self, inc: u64) {
        self.downloaded_bytes.fetch_add(inc, Ordering::SeqCst);
    }
    pub(crate) fn inc_waiting_download_ms(&self, inc: u64) {
        self.waiting_download_ms.fetch_add(inc, Ordering::SeqCst);
    }
    pub(crate) fn inc_download_count(&self, inc: u64) {
        self.download_count.fetch_add(inc, Ordering::SeqCst);
    }
}
