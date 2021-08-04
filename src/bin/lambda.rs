use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use cloud_readers_rs::s3_rusoto::S3FileDescription;
use cloud_readers_rs::{CacheCursor, DownloadCache, Range};
use lambda_runtime::{handler_fn, Context, Error};
use serde::Deserialize;
use serde_json::{json, Value};

static RUN_COUNT: AtomicUsize = AtomicUsize::new(0);

#[tokio::main]
async fn main() -> Result<(), Error> {
    let func = handler_fn(func);
    lambda_runtime::run(func).await?;
    Ok(())
}

#[derive(Deserialize)]
struct Config {
    pub region: String,
    pub bucket: String,
    pub key: String,
    pub size: u64,
    pub max_parallel: usize,
    pub ranges: Vec<Range>,
}

async fn func(event: Value, _: Context) -> Result<Value, Error> {
    let run_count = RUN_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let config: Config = serde_json::from_value(event).unwrap();
    let start_time = Instant::now();
    let file_description =
        S3FileDescription::new(config.region, config.bucket, config.key, config.size);

    let mut download_cache = DownloadCache::new(config.max_parallel);
    let file_manager = download_cache.register(Box::new(file_description)).await;

    file_manager.queue_download(config.ranges.clone())?;

    let mut file_reader = CacheCursor {
        cache: file_manager,
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

    Ok(json!({
        "run_count": run_count,
        "init_duration": init_duration,
        "range_durations": range_durations,
        "downloaded_bytes": download_cache.get_stats().recorded_downloads(),
    }))
}
