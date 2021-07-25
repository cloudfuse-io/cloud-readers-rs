//! Optimized and flexible helpers for reading large data files from cloud storages (or more generally, from the network).
//!
//! To get started, create an instance of `DownloadCache` and start registering `FileHandle` trait objects.

mod cache;

pub use cache::*;
