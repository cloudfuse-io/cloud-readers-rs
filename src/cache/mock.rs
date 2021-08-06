use super::super::{Downloader, FileDescription};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::sync::Arc;

use tokio::time::Duration;

/// A downloader that returns a simple pattern (1,2,3...254,255,1,2...)
/// Waits for 10ms before returning its result to trigger cache misses
struct PatternDownloader;

/// The pattern (1,2,3...254,255,1,2...) in the range [start,end[
pub fn pattern(start: usize, end: usize) -> Vec<u8> {
    (start..end).map(|i| (i % 256) as u8).collect::<Vec<_>>()
}

#[async_trait]
impl Downloader for PatternDownloader {
    async fn download(&self, _file: String, start: u64, length: usize) -> Result<Vec<u8>> {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(pattern(start as usize, start as usize + length))
    }
}

/// A FileDescription implementation that uses [`PatternDownloader`] to generate mock bytes
pub struct PatternFileDescription {
    length: u64,
}

impl PatternFileDescription {
    #[allow(dead_code)]
    pub fn new(length: u64) -> Self {
        Self { length }
    }
}

impl FileDescription for PatternFileDescription {
    fn get_downloader(&self) -> Arc<dyn Downloader> {
        Arc::new(PatternDownloader)
    }

    fn get_downloader_id(&self) -> String {
        "pattern_downloader".to_owned()
    }

    fn get_uri(&self) -> String {
        "pattern_uri".to_owned()
    }

    fn get_file_size(&self) -> u64 {
        self.length
    }
}

/// A downloader that always returns an error
struct ErrorDownloader;

#[async_trait]
impl Downloader for ErrorDownloader {
    async fn download(&self, _file: String, _start: u64, _length: usize) -> Result<Vec<u8>> {
        Err(anyhow!("Download Failed").context("Error in ErrorDownloader"))
    }
}

/// A FileDescription implementation that uses [`ErrorDownloader`] to always fail.
pub struct ErrorFileDescription {}

impl FileDescription for ErrorFileDescription {
    fn get_downloader(&self) -> Arc<dyn Downloader> {
        Arc::new(ErrorDownloader)
    }

    fn get_downloader_id(&self) -> String {
        "error_downloader".to_owned()
    }

    fn get_uri(&self) -> String {
        "error_uri".to_owned()
    }

    fn get_file_size(&self) -> u64 {
        1000
    }
}
