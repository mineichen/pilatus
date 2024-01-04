//!
//! Streams everything the closure(writer) writes to the http-body

use anyhow::Error;
use axum::{body::Body, response::IntoResponse};
use futures::future::FusedFuture;

pub struct IoStreamBody {
    inner: Body,
}

impl IntoResponse for IoStreamBody {
    fn into_response(self) -> axum::response::Response {
        self.inner.into_response()
    }
}

impl IoStreamBody {
    pub fn with_writer<TFut: FusedFuture<Output = Result<(), Error>> + Send + 'static>(
        fut: impl FnOnce(piper::Writer) -> TFut + Send + 'static,
    ) -> Self {
        let (mut rx, tx) = piper::pipe(2000);
        let fut = (fut)(tx);
        Self {
            inner: Body::from_stream(async_stream::stream! {
                let mut fut = std::pin::pin!(fut);

                loop{
                    let mut buf = vec![0; 1500];
                    let mut reader = std::pin::pin!(futures::AsyncReadExt::read(&mut rx, &mut buf[..]));

                    let r = match futures::future::select(&mut fut, &mut reader).await {
                        futures::future::Either::Left((r, other)) => {
                            if let Err(e) = r {
                                yield Result::<Vec<u8>, anyhow::Error>::Err(e);
                                break;
                            }
                            other.await
                        },
                        futures::future::Either::Right((r,_)) => r
                    };
                    match r {
                        Ok(num_bytes) => {
                            if num_bytes == 0 {
                                break;
                            }
                            buf.truncate(num_bytes);
                            yield Ok(buf);

                        }
                        Err(e) => {
                            yield Err(anyhow::Error::from(e));
                            break;
                        }
                    }
                }
            }),
        }
    }
}
