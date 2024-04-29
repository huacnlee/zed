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
