use std::{
    borrow::Borrow,
    error::Error,
    fmt::{self, Display, Formatter},
    io,
    ops::Deref,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use serde::Deserialize;

use super::relative_dir_path::RelativeDirectoryPath;

#[derive(Debug, PartialEq, Eq)]
pub enum RelativeResourcePathError<T> {
    InvalidRelativePath(T),
    InvalidCharacter(T, char, usize),
    FileExtensionMissing(T),
}

impl<T: AsRef<Path>> Display for RelativeResourcePathError<T> {
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
            Self::FileExtensionMissing(t) => {
                write!(f, "Path has no file extension: {}", t.as_ref().display())
            }
        }
    }
}

impl<T: AsRef<Path> + fmt::Debug> Error for RelativeResourcePathError<T> {}

impl<T> RelativeResourcePathError<T> {
    pub fn change_t<U: From<T>>(self) -> RelativeResourcePathError<U> {
        match self {
            Self::InvalidRelativePath(x) => {
                RelativeResourcePathError::InvalidRelativePath(x.into())
            }
            Self::InvalidCharacter(x, y, z) => {
                RelativeResourcePathError::InvalidCharacter(x.into(), y, z)
            }
            Self::FileExtensionMissing(x) => {
                RelativeResourcePathError::FileExtensionMissing(x.into())
            }
        }
    }
}

impl<T> From<RelativeResourcePathError<T>> for io::Error
where
    PathBuf: From<T>,
{
    fn from(value: RelativeResourcePathError<T>) -> Self {
        io::Error::new(io::ErrorKind::InvalidFilename, value.change_t::<PathBuf>())
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct RelativeResourcePath(Path);

impl<'de> Deserialize<'de> for &'de RelativeResourcePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <&Path>::deserialize(deserializer)?;
        RelativeResourcePath::new(s).map_err(<D::Error as serde::de::Error>::custom)
    }
}

impl Deref for RelativeResourcePath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelativeResourcePathBuf(PathBuf);

impl ToOwned for RelativeResourcePath {
    type Owned = RelativeResourcePathBuf;

    fn to_owned(&self) -> Self::Owned {
        RelativeResourcePathBuf(self.0.to_owned())
    }
}

impl Borrow<RelativeResourcePath> for RelativeResourcePathBuf {
    fn borrow(&self) -> &RelativeResourcePath {
        self
    }
}

impl AsRef<RelativeResourcePath> for RelativeResourcePathBuf {
    fn as_ref(&self) -> &RelativeResourcePath {
        RelativeResourcePath::new_unchecked(&self.0)
    }
}

impl AsRef<Path> for RelativeResourcePathBuf {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for RelativeResourcePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Deref for RelativeResourcePathBuf {
    type Target = RelativeResourcePath;

    fn deref(&self) -> &Self::Target {
        RelativeResourcePath::new_unchecked(&self.0)
    }
}

impl<'a> TryFrom<&'a str> for RelativeResourcePathBuf {
    type Error = RelativeResourcePathError<PathBuf>;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl RelativeResourcePath {
    fn new_unchecked(path: &Path) -> &Self {
        unsafe { &*(std::ptr::from_ref(path) as *const RelativeResourcePath) }
    }

    pub fn new<S: AsRef<Path> + ?Sized>(
        value: &S,
    ) -> Result<&Self, RelativeResourcePathError<&'_ Path>> {
        let path = value.as_ref();
        validate(path)?;
        Ok(Self::new_unchecked(path))
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

impl RelativeResourcePathBuf {
    pub fn new(value: impl Into<PathBuf>) -> Result<Self, RelativeResourcePathError<PathBuf>> {
        let buf = value.into();
        validate(buf).map(Self)
    }
}

impl Display for RelativeResourcePathBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        super::format_with_forward_slash(&self.0, f)
    }
}

impl serde::Serialize for RelativeResourcePathBuf {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(&self)
    }
}

impl<'de> Deserialize<'de> for RelativeResourcePathBuf {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = PathBuf::deserialize(deserializer)?;
        RelativeResourcePathBuf::new(s).map_err(<D::Error as serde::de::Error>::custom)
    }
}

impl FromStr for RelativeResourcePathBuf {
    type Err = RelativeResourcePathError<PathBuf>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RelativeResourcePathBuf::new(s)
    }
}

fn validate<T: AsRef<Path>>(value: T) -> Result<T, RelativeResourcePathError<T>> {
    let path = value.as_ref();
    if path.extension().is_none() {
        return Err(RelativeResourcePathError::FileExtensionMissing(value));
    }

    let mut iter = path.components().map(|c| match c {
        Component::Normal(x) => x.to_str(),
        _ => None,
    });

    let filename_part = iter.next_back();
    let folder_offset = {
        let mut offset = 0usize;
        for x in iter {
            match x {
                Some(part) => {
                    for (i, c) in part.chars().enumerate() {
                        match c {
                            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => {}
                            c => {
                                return Err(RelativeResourcePathError::InvalidCharacter(
                                    value,
                                    c,
                                    i + offset,
                                ));
                            }
                        }
                    }
                    offset += part.len() + 1;
                }
                None => return Err(RelativeResourcePathError::InvalidRelativePath(value)),
            }
        }
        offset
    };

    let mut point_ctr = 0;
    for x in filename_part.into_iter() {
        match x {
            Some(part) => {
                for (i, c) in part.chars().enumerate() {
                    match c {
                        'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => {}
                        '.' => {
                            point_ctr += 1;
                            if point_ctr != 1 {
                                return Err(RelativeResourcePathError::InvalidCharacter(
                                    value,
                                    c,
                                    i + folder_offset,
                                ));
                            }
                        }
                        c => {
                            return Err(RelativeResourcePathError::InvalidCharacter(
                                value,
                                c,
                                i + folder_offset,
                            ));
                        }
                    }
                }
            }
            None => return Err(RelativeResourcePathError::InvalidRelativePath(value)),
        }
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level() {
        for (expected, path) in [(0, "test.jpg"), (1, "foo/bar.baz")] {
            let path = RelativeResourcePathBuf::new(path).unwrap();
            assert_eq!(expected, path.levels());
        }
    }

    #[test]
    fn test_valid_path() {
        assert!(RelativeResourcePathBuf::new("img.jpg").is_ok());
        assert!(RelativeResourcePathBuf::new("myfolder/img.jpg").is_ok());
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
                Err(RelativeResourcePathError::InvalidCharacter(
                    PathBuf::from(invalid_string),
                    char,
                    idx
                )),
                RelativeResourcePathBuf::new(invalid_string)
            );
        }
    }

    #[test]
    fn test_missing_file_extension() {
        for invalid_string in ["", "myfolder/img"] {
            assert_eq!(
                Err(RelativeResourcePathError::FileExtensionMissing(
                    PathBuf::from(invalid_string)
                )),
                RelativeResourcePathBuf::new(invalid_string)
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
                Err(RelativeResourcePathError::InvalidRelativePath(
                    PathBuf::from(invalid_string)
                )),
                RelativeResourcePathBuf::new(invalid_string)
            );
        }
    }

    #[test]
    fn borrowed_and_owned_roundtrip() {
        let owned = RelativeResourcePathBuf::new("folder/img.jpg").unwrap();
        let borrowed: &RelativeResourcePath = &owned;
        let owned_again: RelativeResourcePathBuf = borrowed.to_owned();
        assert_eq!(owned, owned_again);
    }

    #[test]
    fn new_borrowed_returns_valid_ref() {
        let borrowed = RelativeResourcePath::new("folder/img.jpg").unwrap();
        assert_eq!(borrowed.file_name(), "img.jpg");
    }
}
