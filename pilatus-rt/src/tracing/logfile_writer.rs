use std::{fs, io, path::PathBuf};

use itertools::Itertools;
use tracing::trace;
use tracing_appender::rolling::RollingFileAppender;
pub(super) struct LogFileWriter<T> {
    inner: T,
    directory: PathBuf,
    files_to_keep: usize,
    cnt: u8,
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
        self.cnt = self.cnt.overflowing_add(1).0;
        if self.cnt == 1 {
            let files = itertools::process_results(
                fs::read_dir(&self.directory)?.map(|f| {
                    let f = f?;
                    let t = f.metadata()?.modified()?;
                    let n = f.path();
                    io::Result::Ok((t, n))
                }),
                |i| i.sorted_by_key(|x| x.0).rev().skip(self.files_to_keep),
            )?;

            for (_, p) in files {
                trace!("deleting log file '{p:?}'");
                fs::remove_file(p)?;
            }
        }

        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{pin_mut, StreamExt};
    use pilatus::visit_directory_files;
    use std::{io::Write, time::Duration};
    use tokio::fs::File;

    #[tokio::test]
    async fn keep_newest_files() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmppath = tmpdir.into_path();

        File::create(tmppath.join("file1.log")).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;
        File::create(tmppath.join("file2.log")).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;
        File::create(tmppath.join("file3.log")).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;

        let mut logfilewriter = LogFileWriter::new(
            tracing_appender::rolling::never(&tmppath, "pilatus-logs"),
            &tmppath,
            2,
        );

        logfilewriter.flush().unwrap();

        let files = visit_directory_files(tmppath);
        pin_mut!(files);

        #[rustfmt::skip]
        assert_eq!("file3.log",files.next().await.unwrap().unwrap().file_name().to_str().unwrap());
        #[rustfmt::skip]
        assert_eq!("pilatus-logs",files.next().await.unwrap().unwrap().file_name().to_str().unwrap());
        assert!(files.next().await.is_none());
    }
}
