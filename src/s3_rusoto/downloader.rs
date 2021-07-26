use std::str::FromStr;
use std::sync::Arc;

use anyhow::{ensure, Context, Result};
use async_trait::async_trait;
use rusoto_core::Region;
use rusoto_s3::{GetObjectOutput, GetObjectRequest, S3Client, S3};
use tokio::io::AsyncReadExt;

use crate::Downloader;

//// Implementation of the `download` function used by the range cache to fetch data

#[derive(Clone)]
pub struct S3Downloader {
    client: Arc<S3Client>,
}

#[async_trait]
impl Downloader for S3Downloader {
    async fn download(&self, uri: String, start: u64, length: usize) -> Result<Vec<u8>> {
        let mut file_id_split = uri.split("/");
        let range = format!("bytes={}-{}", start, start + length as u64 - 1);
        let get_obj_req = GetObjectRequest {
            bucket: file_id_split.next().unwrap().to_owned(),
            key: file_id_split.collect::<Vec<&str>>().join("/"),
            range: Some(range),
            ..Default::default()
        };
        let obj: GetObjectOutput = self
            .client
            .get_object(get_obj_req)
            .await
            .context("Rusoto GetObject error")?;
        let mut reader = obj.body.unwrap().into_async_read();
        let mut res = vec![];
        res.reserve(length);
        let bytes_read = reader
            .read_to_end(&mut res)
            .await
            .context("Rusoto buffer read error")?;
        ensure!(bytes_read == length, "Not the expected number of bytes");
        Ok(res)
    }
}

impl S3Downloader {
    pub fn new(region: &str) -> Self {
        S3Downloader {
            client: new_client(region),
        }
    }
}

fn new_client(region: &str) -> Arc<S3Client> {
    let region = Region::from_str(region).unwrap();
    Arc::new(S3Client::new(region))
}
