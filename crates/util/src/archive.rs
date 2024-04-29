use std::path::Path;

use anyhow::Result;
use async_compression::futures::bufread::GzipDecoder;
use async_tar::Archive;
use async_zip::base::read::stream::ZipFileReader;
use futures::{io::BufReader, AsyncRead};

pub async fn extract_gz<R: AsyncRead + Unpin>(dst: &Path, reader: R) -> Result<()> {
    let decompressed_bytes = GzipDecoder::new(BufReader::new(reader));
    let mut file = smol::fs::File::create(dst).await?;
    futures::io::copy(decompressed_bytes, &mut file).await?;

    Ok(())
}

pub async fn extract_tar_gz<R: AsyncRead + Unpin>(dst: &Path, reader: R) -> Result<()> {
    let decompressed_bytes = GzipDecoder::new(BufReader::new(reader));
    let archive = Archive::new(decompressed_bytes);
    archive.unpack(dst).await?;

    Ok(())
}

pub async fn extract_zip<R: AsyncRead + Unpin>(dst: &Path, reader: R) -> Result<()> {
    let mut reader = ZipFileReader::new(BufReader::new(reader));

    let dst = &dst.canonicalize().unwrap_or_else(|_| dst.to_path_buf());
    while let Some(mut item) = reader.next_with_entry().await? {
        let entry_reader = item.reader_mut();
        let entry = entry_reader.entry();
        let path = dst.join(entry.filename().as_str().unwrap());

        if entry.dir().unwrap() {
            smol::fs::create_dir_all(&path).await?;
        } else {
            let parent_dir = path.parent().expect("failed to get parent directory");
            smol::fs::create_dir_all(&parent_dir).await?;
            let mut file = smol::fs::File::create(&path).await?;
            futures::io::copy(entry_reader, &mut file).await?;
        }

        reader = item.done().await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use smol::io::{BufReader, Cursor};
    use tempfile::NamedTempFile;

    use super::*;

    #[track_caller]
    fn assert_file_content(path: &Path, content: &str) {
        assert!(path.exists(), "file not found: {:?}", path);
        let actual = std::fs::read_to_string(path).unwrap();
        assert_eq!(actual, content);
    }

    #[test]
    fn test_extract_gz() {
        smol::block_on(async {
            let data = include_bytes!("../test_data/test.gz");
            let reader = BufReader::new(Cursor::new(data));
            let file = NamedTempFile::new().unwrap();
            let dst = file.path().with_extension("txt");
            extract_gz(&dst, reader).await.unwrap();

            assert_file_content(&dst, "Hello world.");
            file.close().unwrap();
        });
    }

    #[test]
    fn test_extract_tar_gz() {
        smol::block_on(async {
            let data = include_bytes!("../test_data/test.tar.gz");
            let reader = BufReader::new(Cursor::new(data));
            let dir = tempfile::tempdir().unwrap();
            let dst = dir.path();
            extract_tar_gz(dst, reader).await.unwrap();

            assert_file_content(&dst.join("test"), "Hello world.");
            assert_file_content(&dst.join("foo/bar.txt"), "Foo bar.");
            dir.close().unwrap();
        });
    }

    #[test]
    fn test_extract_zip() {
        smol::block_on(async {
            let data = include_bytes!("../test_data/test.zip");
            let reader = BufReader::new(Cursor::new(data));
            let dir = tempfile::tempdir().unwrap();
            let dst = dir.path();
            extract_zip(dst, reader).await.unwrap();

            assert_file_content(&dst.join("test"), "Hello world.");
            assert_file_content(&dst.join("foo/bar.txt"), "Foo bar.");
            dir.close().unwrap();
        });
    }
}
