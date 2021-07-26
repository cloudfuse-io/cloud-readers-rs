//! [`FileHandle`](`crate::FileHandle`) implementation for downloading from S3 using Rusoto

mod downloader;
mod file_handle;

use downloader::S3Downloader;
pub use file_handle::S3FileHandle;
