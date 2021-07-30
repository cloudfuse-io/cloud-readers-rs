use std::io::{Read, Seek, SeekFrom};
use std::time::Instant;

use cloud_readers_rs::s3_rusoto::S3FileHandle;
use cloud_readers_rs::{DownloadCache, FileCacheCursor, Range};
use lambda_runtime::{handler_fn, Context, Error};
use serde::Deserialize;
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let func = handler_fn(func);
    lambda_runtime::run(func).await?;
    Ok(())
}

#[derive(Deserialize)]
struct ConfigRange {
    start: u64,
    length: usize,
}

#[derive(Deserialize)]
struct Config {
    pub region: String,
    pub bucket: String,
    pub key: String,
    pub size: u64,
    pub max_parallel: usize,
    pub ranges: Vec<ConfigRange>,
}

async fn func(event: Value, _context: Context) -> Result<Value, Error> {
    let config: Config = serde_json::from_value(event).unwrap();
    let start_time = Instant::now();
    let file_handle = S3FileHandle::new(config.region, config.bucket, config.key, config.size);

    let mut download_cache = DownloadCache::new(config.max_parallel);
    let mut file_cache = download_cache.register(Box::new(file_handle)).await;

    file_cache.queue_download(
        config
            .ranges
            .iter()
            .map(|r| Range {
                start: r.start,
                length: r.length,
            })
            .collect(),
    )?;

    let mut file_reader = FileCacheCursor {
        cache: file_cache,
        position: 0,
    };

    let init_duration = start_time.elapsed().as_millis() as u64;

    let mut range_durations = vec![];
    let start_time = Instant::now();
    for range in config.ranges {
        // reading the bytes forces to block until the range is downloaded
        let mut buf = vec![0u8; 10];
        file_reader.seek(SeekFrom::Start(range.start)).unwrap();
        file_reader.read_exact(&mut buf).unwrap();
        range_durations.push(start_time.elapsed().as_millis() as u64);
    }

    Ok(json!({ "init_duration": init_duration, "range_durations": range_durations}))
}
