use std::sync::Arc;

use super::S3Downloader;
use crate::{Downloader, FileHandle};

pub struct S3FileHandle {
    region: String,
    bucket: String,
    key: String,
    size: u64,
}

impl S3FileHandle {
    pub fn new(region: String, bucket: String, key: String, size: u64) -> Self {
        S3FileHandle {
            region,
            bucket,
            key,
            size,
        }
    }
}

impl FileHandle for S3FileHandle {
    fn get_downloader(&self) -> Arc<dyn Downloader> {
        Arc::new(S3Downloader::new(&self.region))
    }

    fn get_downloader_id(&self) -> String {
        format!("s3_rusoto:{}", &self.region)
    }

    fn get_uri(&self) -> String {
        format!("{}/{}", self.bucket, self.key)
    }

    fn get_file_size(&self) -> u64 {
        self.size
    }
}
