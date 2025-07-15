use std::pin::pin;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio_util::bytes::BytesMut;

pub(super) async fn has_same_content(
    a: impl AsyncRead,
    b: impl AsyncRead,
) -> std::io::Result<bool> {
    let mut reader_a = pin!(BufReader::with_capacity(4096, a));
    let mut reader_b = pin!(BufReader::with_capacity(4096, b));

    loop {
        let (buf_a, buf_b) =
            futures::future::try_join(reader_a.fill_buf(), reader_b.fill_buf()).await?;
        // If both buffers are empty, we've reached EOF on both streams
        if buf_a.is_empty() && buf_b.is_empty() {
            return Ok(true);
        }

        // If only one buffer is empty, streams have different lengths
        if buf_a.is_empty() || buf_b.is_empty() {
            return Ok(false);
        }

        // Compare the minimum available data
        let compare_len = buf_a.len().min(buf_b.len());

        if buf_a[..compare_len] != buf_b[..compare_len] {
            return Ok(false);
        }

        // Consume the compared data from both readers
        reader_a.consume(compare_len);
        reader_b.consume(compare_len);
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncRead;

    use super::*;

    #[tokio::test]
    async fn with_multiple_lengths() {
        let a = [0; 4097];
        let b = [0; 4096];
        assert!(
            !has_same_content(std::io::Cursor::new(a), std::io::Cursor::new(b))
                .await
                .unwrap()
        );
    }
    #[tokio::test]
    async fn with_diff_in_first_page() {
        let a = [0; 4098];
        let mut b = [0; 4098];
        b[0] = 1;
        assert!(
            !has_same_content(std::io::Cursor::new(a), std::io::Cursor::new(b))
                .await
                .unwrap()
        );
    }
    #[tokio::test]
    async fn with_diff_in_last_page() {
        let a = [0; 4098];
        let mut b = [0; 4098];
        *b.last_mut().unwrap() = 1;
        assert!(
            !has_same_content(std::io::Cursor::new(a), std::io::Cursor::new(b))
                .await
                .unwrap()
        );
    }
    #[tokio::test]
    async fn with_multiple_same_pages() {
        let a = [0; 4098];
        let b = [0; 4098];
        assert!(
            has_same_content(std::io::Cursor::new(a), std::io::Cursor::new(b))
                .await
                .unwrap()
        );
    }
    #[tokio::test]
    async fn with_different_chunk_sizes() {
        struct ChunkCursor<'a>(usize, &'a [u8]);
        impl<'a> AsyncRead for ChunkCursor<'a> {
            fn poll_read(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                let this: &mut Self = std::pin::Pin::into_inner(self);
                let (before, after) = this
                    .1
                    .split_at(this.0.min(buf.remaining()).min(this.1.len()));
                buf.put_slice(&before);
                this.1 = after;
                std::task::Poll::Ready(Ok(()))
            }
        }

        const SAMPLE_SIZE: usize = 10_000;
        let data = (0..SAMPLE_SIZE)
            .map(|x| (x % 255) as u8)
            .collect::<Vec<_>>();

        assert!(
            has_same_content(ChunkCursor(10, &data), ChunkCursor(11, &data))
                .await
                .unwrap()
        );
    }
}
