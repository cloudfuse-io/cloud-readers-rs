use std::str::FromStr;
use std::sync::Arc;

use super::S3Downloader;
use crate::{Downloader, FileHandle};

use rusoto_core::Region;
use rusoto_s3::S3Client;

type DownloaderFactory = Box<dyn Fn(&str) -> Arc<dyn Downloader> + Send>;

pub struct S3FileHandle {
    region: String,
    bucket: String,
    key: String,
    size: u64,
    downloader_factory: DownloaderFactory,
}

impl S3FileHandle {
    pub fn new(region: String, bucket: String, key: String, size: u64) -> Self {
        S3FileHandle {
            region,
            bucket,
            key,
            size,
            downloader_factory: Box::new(move |reg| Arc::new(S3Downloader::new(new_client(reg)))),
        }
    }
    pub fn set_downloader_factory(&mut self, factory: DownloaderFactory) {
        self.downloader_factory = factory;
    }
}

impl FileHandle for S3FileHandle {
    fn get_downloader(&self) -> Arc<dyn Downloader> {
        (self.downloader_factory)(&self.region)
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

fn new_client(region: &str) -> S3Client {
    let region = Region::from_str(region).unwrap();
    S3Client::new(region)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusoto_core::signature::SignedRequest;
    use rusoto_mock::{MockCredentialsProvider, MockRequestDispatcher};
    use rusoto_s3::S3Client;

    #[tokio::test]
    async fn test_file_handle() {
        let mut file_handle = S3FileHandle::new(
            "test-region-1".to_owned(),
            "test_bucket".to_owned(),
            "test_key".to_owned(),
            1000,
        );
        file_handle.set_downloader_factory(Box::new(|_| {
            let client = S3Client::new_with(
                MockRequestDispatcher::default()
                    .with_body("")
                    .with_request_checker(|request: &SignedRequest| {
                        assert_eq!(request.path, "/test_bucket/test_key");
                        assert_eq!(
                            std::str::from_utf8(&request.headers.get("range").unwrap()[0]).unwrap(),
                            "bytes=50-149"
                        );
                    }),
                MockCredentialsProvider,
                Default::default(),
            );
            Arc::new(S3Downloader::new(client))
        }));

        #[allow(unused_must_use)]
        {
            file_handle
                .get_downloader()
                .download(file_handle.get_uri(), 50, 100)
                .await;
        }
    }
}
