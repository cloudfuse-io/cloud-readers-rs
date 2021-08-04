//! The stats module has two implementations that you can switch with the `stats` feature:
//! - the `sync` module is a shared reference to synchronized statistics collectors
//! - the `noop` module is an empty structure that does not collect any statistics

#[cfg(not(feature = "stats"))]
pub use noop::CacheStats;
#[cfg(feature = "stats")]
pub use sync::CacheStats;

#[allow(dead_code)]
pub mod sync {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    struct InnerCacheStats {
        downloaded_bytes: AtomicU64,
        waiting_download_ms: AtomicU64,
        download_count: AtomicU64,
    }

    /// Reference to a synchronized container with inner statistics of the caching system.
    ///
    /// You can get these stats from the [`DownloadCache`](super::super::DownloadCache) directly.
    /// Clonig this object only clones the reference.
    #[derive(Clone)]
    pub struct CacheStats {
        inner: Arc<InnerCacheStats>,
    }

    impl CacheStats {
        pub(crate) fn new() -> Self {
            Self {
                inner: Arc::new(InnerCacheStats {
                    downloaded_bytes: AtomicU64::new(0),
                    waiting_download_ms: AtomicU64::new(0),
                    download_count: AtomicU64::new(0),
                }),
            }
        }

        pub fn downloaded_bytes(&self) -> u64 {
            self.inner.downloaded_bytes.load(Ordering::Relaxed)
        }
        pub fn waiting_download_ms(&self) -> u64 {
            self.inner.waiting_download_ms.load(Ordering::Relaxed)
        }
        pub fn download_count(&self) -> u64 {
            self.inner.download_count.load(Ordering::Relaxed)
        }

        pub(crate) fn inc_downloaded_bytes(&self, inc: u64) {
            self.inner.downloaded_bytes.fetch_add(inc, Ordering::SeqCst);
        }
        pub(crate) fn inc_waiting_download_ms(&self, inc: u64) {
            self.inner
                .waiting_download_ms
                .fetch_add(inc, Ordering::SeqCst);
        }
        pub(crate) fn inc_download_count(&self, inc: u64) {
            self.inner.download_count.fetch_add(inc, Ordering::SeqCst);
        }
    }
}

#[allow(dead_code)]
pub mod noop {
    /// A structure that does noop when called to collect stats.
    ///
    /// Calls to on this structure will mostly be optimized out by the compiler.
    #[derive(Clone)]
    pub struct CacheStats;

    impl CacheStats {
        pub(crate) fn new() -> Self {
            Self
        }

        pub fn downloaded_bytes(&self) -> u64 {
            0
        }
        pub fn waiting_download_ms(&self) -> u64 {
            0
        }
        pub fn download_count(&self) -> u64 {
            0
        }

        pub(crate) fn inc_downloaded_bytes(&self, _: u64) {}
        pub(crate) fn inc_waiting_download_ms(&self, _: u64) {}
        pub(crate) fn inc_download_count(&self, _: u64) {}
    }
}
