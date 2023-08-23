use serde::{Deserialize, Serialize};

mod angle;
mod frame;
mod invertibletransform;
mod invertibletransform3d;
mod length;
#[cfg(feature = "nalgebra")]
mod nalgebra;
mod relative_polygon;
mod relative_rectangle;

pub use angle::*;
pub use frame::*;
pub use invertibletransform::*;
pub use invertibletransform3d::*;
pub use length::*;
pub use relative_polygon::*;
pub use relative_rectangle::*;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, Copy, Default)]
pub struct XYZ;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, Copy, Default)]
pub struct ZYX;
