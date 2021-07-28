use std::io::Read;

use cloud_readers_rs::s3_rusoto::S3FileHandle;
use cloud_readers_rs::{DownloadCache, FileCacheCursor, Range};
use lambda_runtime::{handler_fn, Context, Error};
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let func = handler_fn(func);
    lambda_runtime::run(func).await?;
    Ok(())
}

async fn func(_event: Value, _context: Context) -> Result<Value, Error> {
    // let first_name = event["firstName"].as_str().unwrap_or("world");

    let file_handle = S3FileHandle::new(
        "us-east-2".to_owned(),
        "cloudfuse-taxi-data".to_owned(),
        "raw_small/2009/01/data.parquet".to_owned(),
        27_301_328,
    );

    let mut download_cache = DownloadCache::new();
    let mut file_cache = download_cache.register(Box::new(file_handle)).await;

    file_cache.queue_download(vec![
        Range {
            start: 0,
            length: 100,
        },
        Range {
            start: 100,
            length: 200,
        },
    ])?;

    let mut file_reader = FileCacheCursor {
        cache: file_cache,
        position: 0,
    };

    let mut buf = vec![0u8; 200];
    file_reader.read_exact(&mut buf)?;

    Ok(json!({ "message": "bytes seem to have been read ;-)" }))
}
