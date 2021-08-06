//! The stats module has two implementations that you can switch with the `stats` feature:
//! - the `sync` module is a shared reference to synchronized statistics collectors
//! - the `noop` module is an empty structure that does not collect any statistics

use serde::Serialize;

/// KPIs relative to one specific range download
#[derive(Clone, Serialize)]
pub struct DownloadStat {
    // range size in bytes
    pub size: u64,
    /// ms spent inside download function
    pub dl_duration: u64,
    /// ms elapsed since registering
    pub dl_start: u64,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(feature = "stats")]
    async fn test_sync() {
        let stats = CacheStats::new();
        for d in mock_data() {
            stats.record_download(d);
        }
        let result = stats.recorded_downloads();
        assert_eq!(result.len(), mock_data().len())
    }

    #[tokio::test]
    #[cfg(not(feature = "stats"))]
    async fn test_noop() {
        let stats = CacheStats::new();
        for d in mock_data() {
            stats.record_download(d);
        }
        let result = stats.recorded_downloads();
        assert_eq!(result.len(), 0)
    }

    fn mock_data() -> Vec<DownloadStat> {
        vec![
            DownloadStat {
                dl_duration: 231,
                dl_start: 4564,
                size: 10000000,
            },
            DownloadStat {
                dl_duration: 123,
                dl_start: 6547,
                size: 10000000,
            },
        ]
    }
}
