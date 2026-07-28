use std::{
    fmt::{self, Debug, Display, Formatter},
    ops::Deref,
};

use serde::{Deserialize, Serialize};

#[derive(PartialEq, PartialOrd, Clone, Copy, Serialize, Deserialize)]
#[serde(try_from = "f64")]
pub struct Percentage(f64);

#[cfg(feature = "impex")]
impl impex::ImpexPrimitive for Percentage {}

#[derive(Debug, thiserror::Error)]
#[error("Invalid percentage {0}")]
pub struct InvalidPercentage(f64);

impl TryFrom<f64> for Percentage {
    type Error = InvalidPercentage;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if ((0.)..=1.).contains(&value) {
            Ok(Percentage(value))
        } else {
            Err(InvalidPercentage(value))
        }
    }
}

impl Percentage {
    pub const fn new(value: f64) -> Option<Self> {
        if value >= 0. && value <= 1. {
            Some(Percentage(value))
        } else {
            None
        }
    }

    pub const fn max() -> Self {
        Percentage(1.0)
    }
    pub const fn min() -> Self {
        Percentage(0.0)
    }

    pub fn fifty() -> Self {
        Percentage(0.5)
    }
}

impl approx::AbsDiffEq for Percentage {
    type Epsilon = f64;

    fn default_epsilon() -> Self::Epsilon {
        f64::EPSILON
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.0.abs_diff_eq(&other.0, epsilon)
    }
}

impl Percentage {
    pub fn value(&self) -> f64 {
        self.0
    }
}

impl Display for Percentage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("{}%", self.0 * 100.))
    }
}

impl Debug for Percentage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self, f)
    }
}

impl std::ops::Mul<f64> for Percentage {
    type Output = f64;

    fn mul(self, rhs: f64) -> Self::Output {
        self.0 * rhs
    }
}
impl std::ops::Mul for Percentage {
    type Output = Percentage;

    fn mul(self, rhs: Self) -> Self::Output {
        Percentage(self.0 * rhs.0)
    }
}

impl Deref for Percentage {
    type Target = f64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_percentage() {
        assert!(Percentage::new(-1.).is_none());
        assert!(Percentage::new(1.1).is_none());
    }
}
