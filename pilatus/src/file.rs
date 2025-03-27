use std::{
    io,
    path::{Path, PathBuf},
};

use futures_util::{stream, Stream, StreamExt};
use tokio::fs::{self, DirEntry};

/// Recursively creates the target if it doesn't exist
pub async fn clone_directory_deep(
    source: impl Into<PathBuf>,
    target: impl AsRef<Path>,
) -> io::Result<()> {
    let source = source.into();
    let target = target.as_ref();

    let mut errors = std::pin::pin!(visit_directory_files(source.clone()));
    while let Some(e) = errors.next().await {
        let source_path = e?.path();
        let relative_path = source_path.strip_prefix(&source).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                anyhow::anyhow!("strip should always work: {e}"),
            )
        })?;
        let target_path = target.join(relative_path);
        tokio::fs::create_dir_all(target_path.parent().expect("File always has a parent")).await?;
        tokio::fs::copy(source_path, target_path).await?;
    }
    Ok(())
}

// Copied from https://stackoverflow.com/questions/56717139/how-to-asynchronously-explore-a-directory-and-its-sub-directories
pub fn visit_directory_files(
    path: impl Into<PathBuf>,
) -> impl Stream<Item = io::Result<DirEntry>> + Send + 'static {
    async fn one_level(path: PathBuf, to_visit: &mut Vec<PathBuf>) -> io::Result<Vec<DirEntry>> {
        let mut dir = fs::read_dir(path).await?;
        let mut files = Vec::new();

        while let Some(child) = dir.next_entry().await? {
            if child.metadata().await?.is_dir() {
                to_visit.push(child.path());
            } else {
                files.push(child)
            }
        }

        Ok(files)
    }

    stream::unfold(vec![path.into()], |mut to_visit| async {
        let path = to_visit.pop()?;
        let file_stream = match one_level(path, &mut to_visit).await {
            Ok(files) => stream::iter(files).map(Ok).left_stream(),
            Err(e) => stream::once(async { Err(e) }).right_stream(),
        };

        Some((file_stream, to_visit))
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn clone_recursive() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let src = dir.path().join("src");
        let target = dir.path().join("target");

        let recursive_subfolder = src.join("foo/bar");
        fs::create_dir_all(&recursive_subfolder).await?;
        let file_a_path = recursive_subfolder.join("a.txt");
        let file_b_path = recursive_subfolder.join("b.txt");

        fs::File::create(src.join("in_root.txt")).await?;
        fs::File::create(file_a_path).await?;
        fs::File::create(file_b_path).await?;

        clone_directory_deep(src, &target).await?;

        assert_eq!(3, visit_directory_files(target).count().await);

        Ok(())
    }
}
