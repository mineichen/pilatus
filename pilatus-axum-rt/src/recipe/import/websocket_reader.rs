use std::{
    fmt::Debug,
    io::{self, ErrorKind},
    pin::Pin,
    task::{self, Poll},
};

use axum::extract::ws::Message;
use futures::{pin_mut, Stream, StreamExt};
use tokio::io::{AsyncRead, ReadBuf};
use tracing::info;

pub(super) struct AsyncWebsocketReader<T>(AsyncWebsocketReaderState<T>);

impl<T> AsyncWebsocketReader<T> {
    pub fn new(socket: T) -> Self {
        Self(AsyncWebsocketReaderState::Init(Some(socket)))
    }
}

impl<T: Stream<Item = Result<Message, axum::Error>> + Unpin + Debug> AsyncRead
    for AsyncWebsocketReader<T>
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let pin = &mut self.0;
        pin_mut!(pin);
        pin.poll_read(cx, buf)
    }
}

/// First reads the data-length and
enum AsyncWebsocketReaderState<T> {
    Init(Option<T>),
    Running(RunningState<T>),
    Finished,
}

struct RunningState<T> {
    remaining: usize,
    buffer: Option<ExtractVec>,
    stream: T,
}

impl<T: Debug + Unpin> AsyncWebsocketReaderState<T> {
    fn from_running_to_finish_unchecked(
        mut self: Pin<&mut Self>,
        old_remaining: usize,
        written_len: usize,
    ) -> Poll<io::Result<()>> {
        let new_remainer = old_remaining - written_len;
        if new_remainer == 0 {
            let mut other = AsyncWebsocketReaderState::Finished;
            std::mem::swap(&mut other, &mut *self);
        } else if let AsyncWebsocketReaderState::Running(state) = &mut *self {
            state.remaining = new_remainer
        } else {
            panic!("from_running_to_finish must be in RunningState")
        }

        Poll::Ready(Ok(()))
    }
}
impl<T: Stream<Item = Result<Message, axum::Error>> + Unpin + Debug> AsyncRead
    for AsyncWebsocketReaderState<T>
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            AsyncWebsocketReaderState::Init(x) => {
                let sock = x
                    .as_mut()
                    .expect("Data is only extracted when going to Ready-State");
                match sock.poll_next_unpin(cx) {
                    Poll::Ready(Some(Ok(Message::Binary(r)))) => {
                        let mut data = [0u8; 8];
                        data.copy_from_slice(&r[..8]);
                        let remaining = u64::from_le_bytes(data) as usize;
                        info!("Start fileupload with bytes: {remaining}");
                        let stream = x.take().expect("Occurs at the end only");
                        *self = AsyncWebsocketReaderState::Running(RunningState {
                            remaining,
                            buffer: None,
                            stream,
                        });
                        self.poll_read(cx, buf)
                    }
                    Poll::Ready(x) => Poll::Ready(Err(io::Error::new(
                        ErrorKind::InvalidData,
                        format!("Expected number, got {x:?}"),
                    ))),
                    Poll::Pending => Poll::Pending,
                }
            }
            AsyncWebsocketReaderState::Running(state) => match &mut state.buffer {
                Some(bytes) => {
                    if bytes.len() > state.remaining {
                        return Poll::Ready(Err(io::Error::new(
                            ErrorKind::InvalidData,
                            "Got more data than expected",
                        )));
                    }

                    let out_capacity = buf.remaining();
                    let to_be_written = bytes.extract_max(out_capacity);
                    let written_len = to_be_written.len();
                    buf.put_slice(to_be_written);

                    if bytes.len() == 0 {
                        state.buffer = None;
                    }
                    let rem = state.remaining;
                    self.from_running_to_finish_unchecked(rem, written_len)
                }
                None => match state.stream.poll_next_unpin(cx) {
                    Poll::Ready(Some(Ok(Message::Binary(bytes)))) => {
                        let written_len = {
                            if bytes.len() > state.remaining {
                                return Poll::Ready(Err(io::Error::new(
                                    ErrorKind::InvalidData,
                                    "Got more data than expected",
                                )));
                            }

                            let capacity = buf.remaining();
                            if capacity >= bytes.len() {
                                buf.put_slice(&bytes[..]);
                                bytes.len()
                            } else {
                                buf.put_slice(&bytes[0..capacity]);
                                state.buffer = Some(ExtractVec::new(bytes.into(), capacity));
                                capacity
                            }
                        };

                        let rem = state.remaining;
                        self.from_running_to_finish_unchecked(rem, written_len)
                    }
                    Poll::Ready(Some(Ok(x))) => Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("Expected binary package, got: {x:?}"),
                    ))),
                    Poll::Ready(None) => Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Stream ended before enought data was received",
                    ))),
                    Poll::Ready(Some(Err(e))) => Poll::Ready(Err(io::Error::other(e))),
                    Poll::Pending => Poll::Pending,
                },
            },
            AsyncWebsocketReaderState::Finished => Poll::Ready(Ok(())),
        }
    }
}

struct ExtractVec {
    pos: usize,
    data: Vec<u8>,
}

impl ExtractVec {
    fn new(data: Vec<u8>, pos: usize) -> Self {
        Self { pos, data }
    }

    fn len(&self) -> usize {
        self.data.len() - self.pos
    }

    fn extract_max(&mut self, capacity: usize) -> &[u8] {
        let cur_pos = self.pos;
        let to_read = (self.data.len() - cur_pos).min(capacity);
        self.pos += to_read;
        &self.data[cur_pos..(cur_pos + to_read)]
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::ws::Message;
    use tokio::io::AsyncReadExt;

    use super::*;

    #[tokio::test]
    async fn read_with_wrong_length_type() {
        let msgs = std::iter::once(Result::<_, axum::Error>::Ok(Message::Text("ABC".into())));
        let stream = futures::stream::iter(msgs);
        let mut state = super::AsyncWebsocketReaderState::Init(Some(stream));

        let mut buffer = Vec::new();
        let error = state.read_to_end(&mut buffer).await.unwrap_err();
        assert!(error.to_string().contains("Expected number"));
    }
    #[tokio::test]
    async fn send_too_much_throws_error() {
        let data = (0..100u8).collect::<Vec<_>>();
        let len_bytes = 99u64.to_le_bytes().to_vec();
        let msgs = std::iter::once(Result::<_, axum::Error>::Ok(Message::Binary(
            len_bytes.into(),
        )))
        .chain(
            data.chunks(10)
                .map(|x| Ok(Message::Binary(x.to_vec().into())))
                .collect::<Vec<_>>(),
        );
        let mut state = super::AsyncWebsocketReaderState::Init(Some(futures::stream::iter(msgs)));

        let mut buffer = Vec::new();
        let error = state.read_to_end(&mut buffer).await.unwrap_err();
        assert!(error.to_string().contains("more data"));
    }

    #[tokio::test]
    async fn read_all() {
        let data = (0..99u8).collect::<Vec<_>>();
        let len_bytes = 99u64.to_le_bytes().to_vec();
        let msgs = std::iter::once(Result::<_, axum::Error>::Ok(Message::Binary(
            len_bytes.into(),
        )))
        .chain(
            data.chunks(10)
                .map(|x| Ok(Message::Binary(x.to_vec().into())))
                .collect::<Vec<_>>(),
        )
        .chain(std::iter::once(Ok(Message::Text("Moore".into()))));
        let mut stream = futures::stream::iter(msgs);
        let mut state = super::AsyncWebsocketReaderState::Init(Some(&mut stream));

        let mut buffer = Vec::new();
        state.read_to_end(&mut buffer).await.unwrap();
        assert_eq!(
            (0..99).sum::<u32>(),
            buffer.into_iter().map(|x| x as u32).sum::<u32>()
        );

        let Some(Ok(Message::Text(x))) = stream.next().await else {
            panic!("Expected more items");
        };
        assert_eq!("Moore", x.as_str());
    }

    #[tokio::test]
    async fn read_big_chunks() {
        const SIZE: usize = 10 * 1024 * 1024;
        let data = vec![1; 10 * 1024 * 1024];
        let len_bytes = (data.len() as u64).to_le_bytes().to_vec();
        let msgs = std::iter::once(Result::<_, axum::Error>::Ok(Message::Binary(
            len_bytes.into(),
        )))
        .chain(
            data.chunks(1024 * 1024)
                .map(|x| Ok(Message::Binary(x.to_vec().into())))
                .collect::<Vec<_>>(),
        );
        let mut state = super::AsyncWebsocketReaderState::Init(Some(futures::stream::iter(msgs)));
        let mut buffer = Vec::new();
        state.read_to_end(&mut buffer).await.unwrap();
        assert_eq!(SIZE, buffer.into_iter().map(|x| x as usize).sum::<usize>());
    }
}
