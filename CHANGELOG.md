#v0.2
- Add `RelativeResourcePath` and `RelativeResourcePathBuf`. Deprecate `RelativeFilePath`, as it's name was misleading (it contained a PathBuf)
- `RelativeDirPathError` and `RelativeFilePathError` changed from containing `String` to the constructor input type
