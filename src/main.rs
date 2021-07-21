use std::sync::Arc;

use anyhow::Result;
use cloud_readers_rs::old::cached_file::CachedFile;
use cloud_readers_rs::old::range_cache::RangeCache;
use cloud_readers_rs::old::s3;
use std::io::Read;

#[tokio::main]
async fn main() -> Result<()> {
    let (dler_id, dler_creator) = s3::downloader_creator("us-east-2");
    let file_id = s3::file_id("cloudfuse-taxi-data", "raw_small/2009/01/data.parquet");
    let length = 27_301_328;
    let cache = Arc::new(RangeCache::new().await);
    let file = CachedFile::new(
        file_id.clone(),
        length,
        Arc::clone(&cache),
        dler_id.clone(),
        dler_creator,
    );

    file.prefetch(0, 100);
    file.prefetch(200, 100);
    file.prefetch(400, 100);

    let mut reader = cache.get(dler_id, file_id, 200, 50)?;

    let mut buf = vec![0u8; 50];
    reader.read_exact(&mut buf)?;

    Ok(())
}
