//! Optimized and flexible helpers for reading large data files from cloud storages (or more generally, from the network).
//!
//! To get started, create an instance of `DownloadCache` and start registering `FileDescription` trait objects.

mod cache;
pub use cache::*;

#[cfg(feature = "s3_rusoto")]
pub mod s3_rusoto;
