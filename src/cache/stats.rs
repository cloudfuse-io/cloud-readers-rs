//! The stats module has two implementations that you can switch with the `stats` feature:
//! - the `sync` module is a shared reference to synchronized statistics collectors
//! - the `noop` module is an empty structure that does not collect any statistics

use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct DownloadStat {
    pub size: u64,
    pub dl_duration: u64,
    pub wait_duration: u64,
}

#[cfg(not(feature = "stats"))]
pub use noop::CacheStats;
#[cfg(feature = "stats")]
pub use sync::CacheStats;

#[allow(dead_code)]
pub mod sync {
    use super::DownloadStat;
    use std::sync::{Arc, Mutex};

    struct InnerCacheStats {
        downloads: Mutex<Vec<DownloadStat>>,
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
                    downloads: Mutex::new(vec![]),
                }),
            }
        }

        pub fn recorded_downloads(&self) -> Vec<DownloadStat> {
            let guard = self.inner.downloads.lock().unwrap();
            guard.clone()
        }

        pub(crate) fn record_download(&self, dl: DownloadStat) {
            let mut guard = self.inner.downloads.lock().unwrap();
            guard.push(dl);
        }
    }
}

#[allow(dead_code)]
pub mod noop {
    use super::DownloadStat;

    /// A structure that does noop when called to collect stats.
    ///
    /// Calls to on this structure will mostly be optimized out by the compiler.
    #[derive(Clone)]
    pub struct CacheStats;

    impl CacheStats {
        pub(crate) fn new() -> Self {
            Self
        }

        pub fn recorded_downloads(&self) -> Vec<DownloadStat> {
            vec![]
        }

        pub(crate) fn record_download(&self, _: DownloadStat) {}
    }
}
