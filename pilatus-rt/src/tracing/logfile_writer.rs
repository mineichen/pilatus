use std::{fs, io, path::PathBuf};

use itertools::Itertools;
use tracing::trace;
use tracing_appender::rolling::RollingFileAppender;
pub(super) struct LogFileWriter<T> {
    inner: T,
    directory: PathBuf,
    files_to_keep: usize,
    cnt: usize,
}
impl<T> LogFileWriter<T> {
    pub(super) fn new(inner: T, d: impl Into<PathBuf>, files_to_keep: usize) -> Self {
        Self {
            inner,
            directory: d.into(),
            files_to_keep,
            cnt: 0,
        }
    }
}
impl io::Write for LogFileWriter<RollingFileAppender> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.cnt += 1;
        if self.cnt > 1000 {
            self.cnt = 0;
            let files = itertools::process_results(
                fs::read_dir(&self.directory)?.map(|f| {
                    let f = f?;
                    let t = f.metadata()?.modified()?;
                    let n = f.path();
                    io::Result::Ok((t, n))
                }),
                |i| i.sorted_by(|a, b| b.0.cmp(&a.0)).skip(self.files_to_keep),
            )?;

            for (_, p) in files {
                trace!("deleting log file '{p:?}'");
                fs::remove_file(p)?;
            }
        }

        self.inner.flush()
    }
}
