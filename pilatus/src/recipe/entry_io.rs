use futures::future::BoxFuture;
use futures::io::AsyncRead;

pub trait PinReader: AsyncRead + Unpin + Send {}
impl<T> PinReader for T where T: AsyncRead + Unpin + Send {}

pub trait EntryWriter: Send {
    fn insert<'a>(
        &'a mut self,
        path: String,
        data: &'a mut dyn PinReader,
    ) -> BoxFuture<'a, std::io::Result<()>>;
    fn close(self: Box<Self>) -> BoxFuture<'static, std::io::Result<()>>;
}

pub trait EntryReader: Send {
    fn next(&mut self) -> BoxFuture<'_, Option<std::io::Result<EntryItem>>>;
}

pub struct EntryItem<'a> {
    pub filename: String,
    pub reader: Box<dyn PinReader + 'a>,
}
