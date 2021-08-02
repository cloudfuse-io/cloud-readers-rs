use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

/// Connection to a server that allows to download chunks of files
#[async_trait]
pub trait Downloader: Send + Sync {
    async fn download(&self, uri: String, start: u64, length: usize) -> Result<Vec<u8>>;
}

/// Start and length of a file chunk
pub struct Range {
    pub start: u64,
    pub length: usize,
}

/// Fully specified description of a downloadable file
///
/// To make it fully generic, a FileDescription provides its own downloader.
/// It also provides a downloader_id to allow the [`DownloadCache`](super::DownloadCache) to cache the downloader.
/// It is not the responsability of the FileDescription to cache the downloader.
/// Downloaders that will most efficiently be re-usable for a given set of files should be assigned the same id.
/// For instance for AWS S3, this will be by region as the domain will be the same, which allows full re-use of
/// the SSL connection (TODO: verify if it is not actually by bucket).
pub trait FileDescription: Send {
    fn get_downloader(&self) -> Arc<dyn Downloader>;

    fn get_downloader_id(&self) -> String;

    fn get_uri(&self) -> String;

    fn get_file_size(&self) -> u64;
}
