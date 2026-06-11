use std::path::PathBuf;

#[deprecated = "Renamed to RelativeResourcePathBuf, because RelativeFilePath contained a PathBuf"]
pub type RelativeFilePath = super::relative_resource_path::RelativeResourcePathBuf;
#[deprecated = "Renamed to RelativeResourcePathError, because RelativeFilePath contained a PathBuf"]
pub type RelativeFilePathError = super::relative_resource_path::RelativeResourcePathError<PathBuf>;
