use std::{
    borrow::Borrow,
    error::Error,
    fmt::{self, Display, Formatter},
    io,
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::Deserialize;

use super::relative_dir_path::RelativeDirectoryPath;

#[derive(Debug, PartialEq, Eq)]
pub enum RelativeResourcePathError<T> {
    InvalidRelativePath(T),
    InvalidCharacter(T, char, usize),
    FileExtensionMissing(T),
    FileStemMissing(T),
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
            Self::FileStemMissing(t) => {
                write!(f, "File stem missing: {}", t.as_ref().display())
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
            Self::FileStemMissing(x) => RelativeResourcePathError::FileStemMissing(x.into()),
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
    const fn new_unchecked(path: &Path) -> &Self {
        unsafe { &*(std::ptr::from_ref(path) as *const RelativeResourcePath) }
    }

    pub fn new<S: AsRef<Path> + ?Sized>(
        value: &S,
    ) -> Result<&Self, RelativeResourcePathError<&'_ Path>> {
        let path = value.as_ref();
        validate(path)?;
        Ok(Self::new_unchecked(path))
    }

    pub const fn new_const(path: &str) -> Option<&Self> {
        if validate_str(path).is_err() {
            return None;
        }
        // Safety: Path is transparent OsStr and Os
        // Str is transparent [u8]. We checked for ansi during validate
        Some(unsafe { &*(std::ptr::from_ref(path) as *const RelativeResourcePath) })
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
    pub fn file_stem(&self) -> &str {
        self.0
            .file_stem()
            .expect("type is not constructable without a filestem")
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValidationFault {
    InvalidRelativePath,
    InvalidCharacter(char, usize),
    FileExtensionMissing,
    FileStemMissing,
}

const fn validate_str(path: &str) -> Result<(), ValidationFault> {
    let bytes = path.as_bytes();
    let len = bytes.len();

    if len == 0 {
        return Err(ValidationFault::FileExtensionMissing);
    }

    let mut i = 0;
    let mut last_point_pos = None;
    let mut alphanumeric_after_last_slash = false;
    while i < len {
        match bytes[i] {
            b'.' => {
                if i == 0 && i + 1 < len && bytes[i + 1] == b'/' {
                    return Err(ValidationFault::InvalidRelativePath);
                } else if let Some(last_point) = last_point_pos {
                    return if last_point + 1 == i {
                        Err(ValidationFault::InvalidRelativePath)
                    } else {
                        Err(ValidationFault::InvalidCharacter('.', i))
                    };
                }
                last_point_pos = Some(i);
            }
            b'/' => {
                if let Some(x) = last_point_pos {
                    return Err(ValidationFault::InvalidCharacter('.', x));
                } else if i == 0 || bytes[i - 1] == b'/' {
                    return Err(ValidationFault::InvalidRelativePath);
                }
                last_point_pos = None;
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' => {
                alphanumeric_after_last_slash = true;
            }
            b => return Err(ValidationFault::InvalidCharacter(b as char, i)),
        }
        i += 1;
    }

    if let (true, Some(x)) = (alphanumeric_after_last_slash, last_point_pos) {
        if x == 0 || bytes[x - 1] == b'/' {
            Err(ValidationFault::FileStemMissing)
        } else {
            Ok(())
        }
    } else {
        Err(ValidationFault::FileExtensionMissing)
    }
}

fn validate<T: AsRef<Path>>(value: T) -> Result<T, RelativeResourcePathError<T>> {
    let Some(as_str) = value.as_ref().to_str() else {
        return Err(RelativeResourcePathError::InvalidRelativePath(value));
    };
    match validate_str(as_str) {
        Ok(()) => Ok(value),
        Err(fault) => Err(match fault {
            ValidationFault::InvalidRelativePath => {
                RelativeResourcePathError::InvalidRelativePath(value)
            }
            ValidationFault::InvalidCharacter(c, i) => {
                RelativeResourcePathError::InvalidCharacter(value, c, i)
            }
            ValidationFault::FileExtensionMissing => {
                RelativeResourcePathError::FileExtensionMissing(value)
            }
            ValidationFault::FileStemMissing => RelativeResourcePathError::FileStemMissing(value),
        }),
    }
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
        assert!(RelativeResourcePathBuf::new("myfolder1/img.jpg").is_ok());
        assert!(RelativeResourcePathBuf::new("my_folder/img.jpg").is_ok());
        assert!(RelativeResourcePathBuf::new("my-folder/img.jpg").is_ok());
    }

    #[test]
    fn test_missing_file_stem() {
        for invalid_string in [".passwd", "folder/sub/.folder"] {
            let as_path = Path::new(invalid_string);
            let x = RelativeResourcePath::new(as_path);
            assert_eq!(x, Err(RelativeResourcePathError::FileStemMissing(as_path)));
        }
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
        for invalid_string in ["", ".", "myfolder/img", "test/"] {
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
            "/",
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

    #[test]
    fn new_const_valid() {
        const PATH: &RelativeResourcePath =
            RelativeResourcePath::new_const("dir_name/img.jpg").unwrap();
        assert_eq!(PATH.file_name(), "img.jpg");

        const NESTED: &RelativeResourcePath =
            RelativeResourcePath::new_const("folder/img.jpg").unwrap();
        assert_eq!(NESTED.file_name(), "img.jpg");

        const DEEP: &RelativeResourcePath =
            RelativeResourcePath::new_const("a/b/c/img.jpg").unwrap();
        assert_eq!(DEEP.file_name(), "img.jpg");
    }
}
