use std::pin::Pin;

use anyhow::Error;
use axum::{body::StreamBody, response::IntoResponse};
use bytes::Bytes;
use futures::{stream::StreamExt,Stream, future::FusedFuture}; 

type  DynBytesStream = dyn Stream<Item = Result<Bytes, Error>> + Send + 'static;
pub struct IoStreamBody {
    inner: StreamBody<Pin<Box<DynBytesStream>>>,
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
            inner: StreamBody::new(
                async_stream::stream! { 
                    let mut fut = std::pin::pin!(fut);
                    
                    loop{
                        let mut buf = vec![0; 1500]; 
                        let mut reader = std::pin::pin!(futures::AsyncReadExt::read(&mut rx, &mut buf[..]));
                       
                        let r = match futures::future::select(&mut fut, &mut reader).await {
                            futures::future::Either::Left((r, other)) => {
                                if let Err(e) = r {
                                    yield Err(e);
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
                                yield Ok(buf.into());

                            }
                            Err(e) => {
                                tracing::error!("Error streaming logs-zip: {e}");
                                yield Err(anyhow::Error::from(e));
                                break;
                            }
                        }
                    }
                }
                .boxed(),
            )
        }
    }
}
