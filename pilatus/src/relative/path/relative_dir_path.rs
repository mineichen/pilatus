use std::{
    borrow::Borrow,
    fmt::{self, Display, Formatter},
    io,
    ops::Deref,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use serde::Deserialize;

#[derive(Debug, PartialEq, Eq)]
pub enum RelativeDirPathError<T> {
    InvalidRelativePath(T),
    InvalidCharacter(T, char, usize),
}

impl<T: AsRef<Path>> Display for RelativeDirPathError<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRelativePath(t) => {
                write!(f, "Invalid relative Path: {}", t.as_ref().display())
            }
            Self::InvalidCharacter(t, c, i) => {
                write!(
                    f,
                    "Invalid character '{c}' in Path '{}' at position {i}",
                    t.as_ref().display(),
                )
            }
        }
    }
}

impl<T: AsRef<Path> + fmt::Debug> std::error::Error for RelativeDirPathError<T> {}

impl<T> RelativeDirPathError<T> {
    pub fn change_t<U: From<T>>(self) -> RelativeDirPathError<U> {
        match self {
            Self::InvalidRelativePath(x) => RelativeDirPathError::InvalidRelativePath(x.into()),
            Self::InvalidCharacter(x, y, z) => {
                RelativeDirPathError::InvalidCharacter(x.into(), y, z)
            }
        }
    }
}

impl<T> From<RelativeDirPathError<T>> for io::Error
where
    PathBuf: From<T>,
{
    fn from(value: RelativeDirPathError<T>) -> Self {
        io::Error::new(io::ErrorKind::InvalidFilename, value.change_t::<PathBuf>())
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct RelativeDirectoryPath(Path);

impl<'de> Deserialize<'de> for &'de RelativeDirectoryPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <&Path>::deserialize(deserializer)?;
        RelativeDirectoryPath::new(s)
            .map_err(|e| <D::Error as serde::de::Error>::custom(e.change_t::<PathBuf>()))
    }
}

impl Deref for RelativeDirectoryPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelativeDirectoryPathBuf(PathBuf);

impl ToOwned for RelativeDirectoryPath {
    type Owned = RelativeDirectoryPathBuf;

    fn to_owned(&self) -> Self::Owned {
        RelativeDirectoryPathBuf(self.0.to_owned())
    }
}

impl Borrow<RelativeDirectoryPath> for RelativeDirectoryPathBuf {
    fn borrow(&self) -> &RelativeDirectoryPath {
        self
    }
}

impl AsRef<RelativeDirectoryPath> for RelativeDirectoryPathBuf {
    fn as_ref(&self) -> &RelativeDirectoryPath {
        RelativeDirectoryPath::new_unchecked(&self.0)
    }
}

impl AsRef<Path> for RelativeDirectoryPathBuf {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for RelativeDirectoryPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Deref for RelativeDirectoryPathBuf {
    type Target = RelativeDirectoryPath;

    fn deref(&self) -> &Self::Target {
        RelativeDirectoryPath::new_unchecked(&self.0)
    }
}

impl RelativeDirectoryPath {
    pub(super) fn new_unchecked(path: &Path) -> &Self {
        // safety: RelativeDirectoryPath is repr(transparent)
        unsafe { &*(std::ptr::from_ref(path) as *const RelativeDirectoryPath) }
    }
    pub fn new<S: AsRef<Path> + ?Sized>(value: &S) -> Result<&Self, RelativeDirPathError<&'_ S>> {
        let buf = validate(value)?;
        Ok(Self::new_unchecked(buf.as_ref()))
    }

    pub fn levels(&self) -> usize {
        self.components().count()
    }

    pub fn join(&self, relative_path: &RelativeDirectoryPath) -> RelativeDirectoryPathBuf {
        RelativeDirectoryPathBuf(self.0.join(relative_path))
    }

    pub fn root() -> &'static Self {
        Self::new_unchecked(Path::new(""))
    }
}

impl RelativeDirectoryPathBuf {
    pub fn new(value: impl Into<PathBuf>) -> Result<Self, RelativeDirPathError<PathBuf>> {
        let buf = value.into();
        let buf = validate(buf)?;
        Ok(Self(buf))
    }
    pub fn root() -> Self {
        Self(PathBuf::new())
    }
}

impl Display for RelativeDirectoryPathBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        super::format_with_forward_slash(&self.0, f)
    }
}

impl serde::Serialize for RelativeDirectoryPathBuf {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(&self)
    }
}

impl<'de> Deserialize<'de> for RelativeDirectoryPathBuf {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = PathBuf::deserialize(deserializer)?;
        RelativeDirectoryPathBuf::new(s).map_err(<D::Error as serde::de::Error>::custom)
    }
}

impl FromStr for RelativeDirectoryPathBuf {
    type Err = RelativeDirPathError<PathBuf>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RelativeDirectoryPathBuf::new(s)
    }
}

fn validate<T: AsRef<Path>>(value: T) -> Result<T, RelativeDirPathError<T>> {
    let path = value.as_ref();

    let iter = path.components().map(|c| match c {
        Component::Normal(x) => x.to_str(),
        _ => None,
    });

    let mut offset = 0usize;
    for x in iter {
        match x {
            Some(part) => {
                for (i, c) in part.chars().enumerate() {
                    match c {
                        'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => {}
                        c => {
                            return Err(RelativeDirPathError::InvalidCharacter(
                                value,
                                c,
                                i + offset,
                            ));
                        }
                    }
                }
                offset += part.len() + 1;
            }
            None => return Err(RelativeDirPathError::InvalidRelativePath(value)),
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level() {
        for (expected, path) in [(0, ""), (1, "test"), (2, "test/jpg")] {
            let path = RelativeDirectoryPathBuf::new(path).unwrap();
            assert_eq!(expected, path.levels());
        }
    }

    #[test]
    fn root_is_valid() {
        let root = RelativeDirectoryPath::root().deref();
        RelativeDirectoryPathBuf::new(root).expect("to be valid");
    }

    #[test]
    fn test_invalid_characters() {
        for (invalid_string, char, idx) in [
            ("fold.er/img", '.', 4),
            ("myfolder/points.jpg", '.', 15),
            #[cfg(not(target_os = "windows"))]
            ("C:/myfolder/img.jpg", ':', 1),
            ("folder/img@jpg", '@', 10),
        ] {
            assert_eq!(
                Err(RelativeDirPathError::InvalidCharacter(
                    invalid_string,
                    char,
                    idx
                )),
                validate(invalid_string)
            );
        }
    }

    #[test]
    fn test_invalid_relative_path() {
        for invalid_string in [
            "/myfolder/img",
            #[cfg(target_os = "windows")]
            "C:/myfolder/img",
            "test/../../myfolder",
            "../myfolder/img",
            "./myfolder/img",
            ".",
        ] {
            assert_eq!(
                Err(RelativeDirPathError::InvalidRelativePath(invalid_string)),
                validate(invalid_string)
            );
        }
    }
}
