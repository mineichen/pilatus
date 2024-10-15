use std::{
    borrow::Borrow,
    fmt::{self, Display, Formatter},
    ops::Deref,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use serde::Deserialize;

use super::RelativePathError;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RelativeDirPathError {
    #[error("Invalid relative Path: {0}")]
    InvalidRelativePath(String),

    #[error("Invalid character '{1}' in Path '{0}' at position {2}")]
    InvalidCharacter(String, char, usize),
}

impl RelativePathError for RelativeDirPathError {
    fn adapt_position(self, offset: usize) -> Self {
        match self {
            RelativeDirPathError::InvalidCharacter(t, c, idx) => {
                RelativeDirPathError::InvalidCharacter(t, c, idx + offset)
            }
            r => r,
        }
    }

    fn from_invalid_char(path: &Path, c: char, idx: usize) -> Self {
        RelativeDirPathError::InvalidCharacter(path.to_string_lossy().to_string(), c, idx)
    }

    fn from_invalid_path(path: &Path) -> Self {
        RelativeDirPathError::InvalidRelativePath(path.to_string_lossy().to_string())
    }
}
#[repr(transparent)]
pub struct RelativeDirectoryPath(Path);

impl<'de> Deserialize<'de> for &'de RelativeDirectoryPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <&Path>::deserialize(deserializer)?;
        RelativeDirectoryPath::new(s).map_err(<D::Error as serde::de::Error>::custom)
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
        &self
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
    pub fn new<'a, S: AsRef<Path> + ?Sized>(
        value: &'a S,
    ) -> Result<&'a Self, RelativeDirPathError> {
        let buf = value.as_ref();
        validate(&buf)?;
        Ok(Self::new_unchecked(buf))
    }

    pub fn levels(&self) -> usize {
        self.components().count()
    }

    pub fn root() -> &'static Self {
        Self::new_unchecked(Path::new(""))
    }
}

impl RelativeDirectoryPathBuf {
    pub fn new(value: impl Into<PathBuf>) -> Result<Self, RelativeDirPathError> {
        let buf = value.into();
        validate(&buf)?;
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
    type Err = RelativeDirPathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RelativeDirectoryPathBuf::new(s)
    }
}

fn validate(value: impl AsRef<Path>) -> Result<(), RelativeDirPathError> {
    let path = value.as_ref();

    super::validate_parts::<RelativeDirPathError>(
        path.components().map(|c| match c {
            Component::Normal(x) => x.to_str(),
            _ => None,
        }),
        |(i, c)| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => Ok(()),
            c => Err((c, i)),
        },
        path,
    )?;

    Ok(())
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
                    invalid_string.to_string(),
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
                Err(RelativeDirPathError::InvalidRelativePath(
                    invalid_string.to_string()
                )),
                validate(invalid_string)
            );
        }
    }
}
