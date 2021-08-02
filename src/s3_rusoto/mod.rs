//! [`FileDescription`](`crate::FileDescription`) implementation for downloading from S3 using Rusoto

mod downloader;
mod file_description;

use downloader::S3Downloader;
pub use file_description::S3FileDescription;
