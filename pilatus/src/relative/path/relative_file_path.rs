use std::{
    fmt::{self, Display, Formatter},
    ops::Deref,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

use super::{relative_dir_path::RelativeDirectoryPath, RelativePathError};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RelativeFilePathError {
    #[error("Invalid relative Path: {0}")]
    InvalidRelativePath(String),

    #[error("Invalid character '{1}' in Path '{0}' at position {2}")]
    InvalidCharacter(String, char, usize),
    #[error("Path has no file extension: {0}")]
    FileExtensionMissing(String),
}

impl RelativePathError for RelativeFilePathError {
    fn adapt_position(self, offset: usize) -> Self {
        match self {
            RelativeFilePathError::InvalidCharacter(t, c, idx) => {
                RelativeFilePathError::InvalidCharacter(t, c, idx + offset)
            }
            r => r,
        }
    }

    fn from_invalid_char(path: &Path, c: char, idx: usize) -> Self {
        RelativeFilePathError::InvalidCharacter(path.to_string_lossy().to_string(), c, idx)
    }

    fn from_invalid_path(path: &Path) -> Self {
        RelativeFilePathError::InvalidRelativePath(path.to_string_lossy().to_string())
    }
}

/// Contains Alphanumeric characters only plus - and _.
/// File must contain exactly one dot (.)
/// File always starts with the folder name (never ./)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelativeFilePath(PathBuf);

impl Deref for RelativeFilePath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> TryFrom<&'a str> for RelativeFilePath {
    type Error = RelativeFilePathError;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl RelativeFilePath {
    pub fn new(value: impl Into<PathBuf>) -> Result<RelativeFilePath, RelativeFilePathError> {
        let buf = value.into();
        validate(&buf)?;
        Ok(RelativeFilePath(buf))
    }

    pub fn relative_dir(&self) -> &RelativeDirectoryPath {
        self.0
            .parent()
            .map(RelativeDirectoryPath::new_unchecked)
            .unwrap_or(RelativeDirectoryPath::root())
    }

    pub fn levels(&self) -> usize {
        self.components().count() - 1
    }

    pub fn file_name(&self) -> &str {
        self.0
            .file_name()
            .expect("type is not constructable without a filename")
            .to_str()
            .expect("type is not constructable without valid utf8")
    }
    pub fn get_path(&self) -> &Path {
        &self.0
    }
}

impl Display for RelativeFilePath {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        super::format_with_forward_slash(&self.0, f)
    }
}

impl<'de> Deserialize<'de> for RelativeFilePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = PathBuf::deserialize(deserializer)?;
        RelativeFilePath::new(s).map_err(<D::Error as serde::de::Error>::custom)
    }
}

impl FromStr for RelativeFilePath {
    type Err = RelativeFilePathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RelativeFilePath::new(s)
    }
}

fn validate(value: impl AsRef<Path>) -> Result<(), RelativeFilePathError> {
    let path = value.as_ref();
    if path.extension().is_none() {
        return Err(RelativeFilePathError::FileExtensionMissing(
            path.to_string_lossy().to_string(),
        ));
    }

    let mut iter = path.components().map(|c| match c {
        Component::Normal(x) => x.to_str(),
        _ => None,
    });

    let filename_result = {
        let mut point_ctr = 0;
        super::validate_parts::<RelativeFilePathError>(
            iter.next_back().into_iter(),
            |(i, c)| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => Ok(()),
                '.' => {
                    point_ctr += 1;
                    if point_ctr == 1 {
                        Ok(())
                    } else {
                        Err((c, i))
                    }
                }
                c => Err((c, i)),
            },
            path,
        )
    };
    let folder_offset = super::validate_parts::<RelativeFilePathError>(
        iter,
        |(i, c)| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => Ok(()),
            c => Err((c, i)),
        },
        path,
    )?;
    filename_result.map_err(|x| x.adapt_position(folder_offset))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level() {
        for (expected, path) in [(0, "test.jpg"), (1, "foo/bar.baz"), (1, "test/./foo.jpg")] {
            let path = RelativeFilePath::new(path).unwrap();
            assert_eq!(expected, path.levels());
        }
    }

    #[test]
    fn test_valid_path() {
        let valid_string = "img.jpg".to_string();
        assert_eq!(Ok(()), validate(valid_string));

        let valid_string = "myfolder/img.jpg".to_string(); //<-valid
        assert_eq!(Ok(()), validate(valid_string));
    }

    #[test]
    fn test_invalid_characters() {
        for (invalid_string, char, idx) in [
            ("fold.er/img.jpg", '.', 4),
            ("myfolder/twopoints.jpg.png", '.', 22),
            ("folder/sub.folder/img.jpg", '.', 10),
            ("f@lder/img.jpg", '@', 1),
            #[cfg(not(target_os = "windows"))]
            ("C:/myfolder/img.jpg", ':', 1),
            ("folder/subfolder/im%g.jpg", '%', 19),
        ] {
            assert_eq!(
                Err(RelativeFilePathError::InvalidCharacter(
                    invalid_string.to_string(),
                    char,
                    idx
                )),
                validate(invalid_string)
            );
        }
    }

    #[test]
    fn test_missing_file_extension() {
        for invalid_string in ["", "myfolder/img"] {
            assert_eq!(
                Err(RelativeFilePathError::FileExtensionMissing(
                    invalid_string.to_string()
                )),
                validate(invalid_string)
            );
        }
    }

    #[test]
    fn test_invalid_relative_path() {
        for invalid_string in [
            "/myfolder/img.jpg",
            #[cfg(target_os = "windows")]
            "C:/myfolder/img.jpg",
            "test/../../myfolder/img.jpg",
            "../myfolder/img.jpg",
            "./myfolder/img.jpg",
        ] {
            assert_eq!(
                Err(RelativeFilePathError::InvalidRelativePath(
                    invalid_string.to_string()
                )),
                validate(invalid_string)
            );
        }
    }
}
