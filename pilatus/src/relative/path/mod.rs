mod relative_dir_path;
mod relative_file_path;

use std::{
    fmt::{self, Formatter},
    path::{Component, Path},
};

pub use relative_dir_path::{RelativeDirPath, RelativeDirPathError};
pub use relative_file_path::{RelativeFilePath, RelativeFilePathError};

trait RelativePathError {
    fn from_invalid_char(path: &Path, c: char, idx: usize) -> Self;
    fn adapt_position(self, offset: usize) -> Self;
    fn from_invalid_path(path: &Path) -> Self;
}

fn format_with_forward_slash(buf: &Path, f: &mut Formatter<'_>) -> fmt::Result {
    let mut iter = buf.components();
    if let Some(Component::Normal(x)) = iter.next() {
        f.write_str(x.to_str().expect("Validaton allows convertables only"))?;
        while let Some(Component::Normal(x)) = iter.next() {
            f.write_str("/")?;
            f.write_str(x.to_str().expect("Validaton allows convertables only"))?;
        }
    }
    Ok(())
}

fn validate_parts<'a, TError: RelativePathError>(
    iter: impl Iterator<Item = Option<&'a str>>,
    mut inner: impl (FnMut((usize, char)) -> Result<(), (char, usize)>) + 'a,
    path: &'a Path,
) -> Result<usize, TError> {
    let mut offset = 0;

    for x in iter.map(move |part| match part {
        Some(x) => x
            .chars()
            .enumerate()
            .try_for_each(&mut inner)
            .map(|_| x.len() + 1)
            .map_err(|(c, i)| TError::from_invalid_char(path, c, i)),
        None => Err(TError::from_invalid_path(path)),
    }) {
        match x {
            Ok(x) => offset += x,
            Err(e) => return Err(e.adapt_position(offset)),
        }
    }
    Ok(offset)
}
