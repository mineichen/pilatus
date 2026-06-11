mod relative_dir_path;
mod relative_file_path;
mod relative_resource_path;

use std::{
    fmt::{self, Formatter},
    path::{Component, Path},
};

pub use relative_dir_path::*;
pub use relative_file_path::*;
pub use relative_resource_path::*;

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
