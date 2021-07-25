use super::super::{Downloader, FileHandle};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

use tokio::time::Duration;

/// A downloader that returns a simple pattern (1,2,3...254,255,1,2...)
/// Waits for 10ms before returning its result to trigger cache misses
struct MockDownloader;

/// The pattern (1,2,3...254,255,1,2...) in the range [start,end[
pub fn pattern(start: usize, end: usize) -> Vec<u8> {
    (start..end).map(|i| (i % 256) as u8).collect::<Vec<_>>()
}

#[async_trait]
impl Downloader for MockDownloader {
    async fn download(&self, _file: String, start: u64, length: usize) -> Result<Vec<u8>> {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(pattern(start as usize, start as usize + length))
    }
}

/// A FileHandle implementation that uses [`MockDownloader`] to generate mock bytes
pub struct MockFileHandle {
    length: u64,
}

impl MockFileHandle {
    #[allow(dead_code)]
    pub fn new(length: u64) -> Self {
        Self { length }
    }
}

impl FileHandle for MockFileHandle {
    fn get_downloader(&self) -> Arc<dyn Downloader> {
        Arc::new(MockDownloader)
    }

    fn get_downloader_id(&self) -> String {
        "mock_downloader".to_owned()
    }

    fn get_uri(&self) -> String {
        "mock_uri".to_owned()
    }

    fn get_file_size(&self) -> u64 {
        self.length
    }
}
