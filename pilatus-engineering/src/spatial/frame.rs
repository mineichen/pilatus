use std::{
    fmt::{self, Display, Formatter},
    marker::PhantomData,
};

use serde::{Deserialize, Serialize};

use crate::{Angle, Length, XYZ};

#[derive(
    Serialize, Deserialize, PartialEq, Debug, Clone, Copy, Default, sealedstruct::IntoSealed,
)]
#[serde(deny_unknown_fields)]
pub struct Frame<T = XYZ> {
    pub x: Length,
    pub y: Length,
    pub z: Length,
    pub rx: Angle,
    pub ry: Angle,
    pub rz: Angle,
    #[serde(skip)]
    phantom: PhantomData<T>,
}

impl Display for Frame {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "x:{}mm, y:{}mm, z:{}mm, rx:{}°, ry:{}°, rz:{}°",
            self.x.mm_str(),
            self.y.mm_str(),
            self.z.mm_str(),
            self.rx.value_deg_str(),
            self.ry.value_deg_str(),
            self.rz.value_deg_str()
        )
    }
}

impl<T> Frame<T> {
    pub fn new(x: Length, y: Length, z: Length, rx: Angle, ry: Angle, rz: Angle) -> Self {
        Self {
            x,
            y,
            z,
            rx,
            ry,
            rz,
            phantom: PhantomData,
        }
    }

    pub fn values(&self) -> Frame {
        Frame {
            x: self.x,
            y: self.y,
            z: self.z,
            rx: self.rx,
            ry: self.ry,
            rz: self.rz,
            phantom: PhantomData,
        }
    }
}

impl approx::AbsDiffEq for Frame {
    type Epsilon = f64;

    fn default_epsilon() -> Self::Epsilon {
        f64::default()
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.x == other.x
            && self.y == other.y
            && self.z == other.z
            && self.rx.abs_diff_eq(&other.rx, epsilon)
            && self.ry.abs_diff_eq(&other.ry, epsilon)
            && self.rz.abs_diff_eq(&other.rz, epsilon)
    }
}
