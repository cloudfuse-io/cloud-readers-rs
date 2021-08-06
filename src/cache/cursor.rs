use std::io::{self, Read, Seek, SeekFrom};

use anyhow::anyhow;

use super::FileManager;

/// Cursor that allows the to Read/Seek through a [`FileManager`].
///
/// Blocks if bytes are read that were not yet downloaded
/// Fails if bytes are read that were not scheduled for downloading
#[derive(Clone)]
pub struct CacheCursor {
    pub cache: FileManager,
    pub position: u64,
}

impl Read for CacheCursor {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.position == self.cache.get_file_size() {
            return Ok(0);
        }
        let cache_cursor = self
            .cache
            .get_range(self.position)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let bytes_read = cache_cursor.read(buf);

        // update reader position
        self.position += bytes_read as u64;
        Ok(bytes_read)
    }
}

impl Seek for CacheCursor {
    /// implementation inspired from std::io::Cursor
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        let (base_pos, offset) = match pos {
            SeekFrom::Start(n) => {
                self.position = n;
                return Ok(n);
            }
            SeekFrom::End(n) => (self.cache.get_file_size(), n),
            SeekFrom::Current(n) => (self.position, n),
        };
        let new_pos = if offset >= 0 {
            base_pos.checked_add(offset as u64)
        } else {
            base_pos.checked_sub((offset.wrapping_neg()) as u64)
        };
        match new_pos {
            Some(n) => {
                self.position = n;
                Ok(self.position)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                anyhow!("invalid seek to a negative or overflowing position"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{mock::*, DownloadCache, Range};
    use super::*;

    use std::io::{self, Read, Seek, SeekFrom};

    #[tokio::test]
    async fn test_read_within_range() {
        let (download_cache, file_manager) = init_mock(1000).await;

        file_manager
            .queue_download(vec![Range {
                start: 0,
                length: 100,
            }])
            .expect("Could not queue Range on handle");

        let mut cursor = CacheCursor {
            cache: file_manager,
            position: 0,
        };

        assert_cursor(&cursor, pattern(0, 50))
            .await
            .expect("could not read enough bytes [0:50[ in download [0:100[");
        cursor
            .seek(SeekFrom::Start(20))
            .expect("Cursor could not be moved");
        assert_cursor(&cursor, pattern(20, 70))
            .await
            .expect("Could not read bytes bytes [20:70[ in download [0:100[");

        // check the debug output for the cache
        assert_eq!(
            format!("{:?}", download_cache),
            "\
file = pattern_downloader / pattern_uri:
-- Start=0000000000 Status=Done(100 bytes)"
        );
    }

    #[tokio::test]
    async fn test_read_accross_ranges() {
        let (download_cache, file_manager) = init_mock(1000).await;

        file_manager
            .queue_download(vec![
                Range {
                    start: 200,
                    length: 100,
                },
                Range {
                    start: 0,
                    length: 100,
                },
                Range {
                    start: 100,
                    length: 100,
                },
            ])
            .expect("Could not queue Range on handle");

        let cursor = CacheCursor {
            cache: file_manager,
            position: 0,
        };

        assert_cursor(&cursor, pattern(0, 300))
            .await
            .expect("Could not read bytes bytes [0:300[ in download [0:100[+[100:200[+[200:300[");

        // check the debug output for the cache
        assert_eq!(
            format!("{:?}", download_cache),
            "\
file = pattern_downloader / pattern_uri:
-- Start=0000000000 Status=Done(100 bytes)
-- Start=0000000100 Status=Done(100 bytes)
-- Start=0000000200 Status=Done(100 bytes)"
        );
    }

    #[tokio::test]
    async fn test_read_complete_file() {
        let file_size = 100;
        let (download_cache, file_manager) = init_mock(file_size).await;

        // we schedule the download of the whole file
        file_manager
            .queue_download(vec![Range {
                start: 0,
                length: file_size as usize,
            }])
            .expect("Could not queue Range on handle");

        let mut cursor = CacheCursor {
            cache: file_manager,
            position: 0,
        };

        let target = pattern(0, file_size as usize);
        // perform blocking read in separate thread!
        let result = tokio::task::spawn_blocking(move || -> io::Result<Vec<u8>> {
            let mut content = vec![];
            // we try to read the whole file
            cursor.read_to_end(&mut content)?;
            Ok(content)
        })
        .await
        .unwrap()
        .expect(&format!(
            "Could not read bytes bytes [0:{len}[ in download [0:{len}[",
            len = file_size,
        ));
        assert_eq!(result, target);

        // check the debug output for the cache
        assert_eq!(
            format!("{:?}", download_cache),
            "\
file = pattern_downloader / pattern_uri:
-- Start=0000000000 Status=Done(100 bytes)"
        );
    }

    #[tokio::test]
    async fn test_read_uninit() {
        let (_, file_manager) = init_mock(1000).await;

        let mut cursor = CacheCursor {
            cache: file_manager.clone(),
            position: 0,
        };

        // try reading whithout starting any download
        let err_msg = assert_cursor(&cursor, pattern(0, 50))
            .await
            .expect_err("Read should fail if no download was scheduled")
            .to_string();
        assert_eq!(err_msg, "No download scheduled");

        // try reading before of downloaded range
        file_manager
            .queue_download(vec![Range {
                start: 200,
                length: 100,
            }])
            .expect("Could not queue Range on handle");
        let err_msg = assert_cursor(&cursor, pattern(0, 50))
            .await
            .expect_err("Read of [0:50[ should fail, only [200,300[ was downloaded")
            .to_string();
        assert_eq!(
            err_msg,
            "\
Download not scheduled at position 0, scheduled ranges are:
-- Start=0000000200 Status=Pending(100 bytes)"
        );

        // try reading after of downloaded range
        cursor
            .seek(SeekFrom::Start(280))
            .expect("Cursor could not be moved");
        let err_msg = assert_cursor(&cursor, pattern(0, 50))
            .await
            .expect_err("Read of [280:320[ should fail, only [200,300[ was downloaded")
            .to_string();
        assert_eq!(
            err_msg,
            "\
Download not scheduled at position 300, scheduled ranges are:
-- Start=0000000200 Status=Done(100 bytes)"
        );
    }

    #[tokio::test]
    async fn test_seek() {
        let mut cursor = CacheCursor {
            cache: init_mock(100).await.1,
            position: 0,
        };

        cursor.seek(SeekFrom::Start(10)).unwrap();

        assert_eq!(cursor.position, 10);

        cursor.seek(SeekFrom::Current(10)).unwrap();

        assert_eq!(cursor.position, 20);

        cursor.seek(SeekFrom::End(-10)).unwrap();

        assert_eq!(cursor.position, 90);
    }

    //// Test Fixtures: ////

    async fn init_mock(len: u64) -> (DownloadCache, FileManager) {
        let mut download_cache = DownloadCache::new(2);

        let mock_file_description = PatternFileDescription::new(len);

        let file_manager = download_cache
            .register(Box::new(mock_file_description))
            .await;

        (download_cache, file_manager)
    }

    /// Assert that the next `target.len()` bytes of `cursor` match the bytes in `target`
    /// SPAWNS A NEW THREAD TO PERFORM THE READ BECAUSE IT IS BLOCKING!
    async fn assert_cursor(cursor: &CacheCursor, target: Vec<u8>) -> io::Result<()> {
        let target_length = target.len();
        let mut cursor = cursor.clone();
        // perform blocking read in separate thread!
        let result = tokio::task::spawn_blocking(move || -> io::Result<Vec<u8>> {
            let mut content = vec![0u8; target_length];
            cursor.read_exact(&mut content)?;
            Ok(content)
        })
        .await
        .unwrap()?;
        assert_eq!(result, target);
        Ok(())
    }
}
