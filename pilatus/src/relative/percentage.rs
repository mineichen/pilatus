use std::{
    fmt::{self, Debug, Display, Formatter},
    ops::Deref,
};

use serde::{Deserialize, Serialize};

#[derive(PartialEq, PartialOrd, Clone, Copy, Serialize, Deserialize, sealedstruct::Seal)]
pub struct PercentageRaw(f64);

#[cfg(feature = "impex")]
impl impex::ImpexPrimitive for Percentage {}

impl sealedstruct::Validator for PercentageRaw {
    fn check(&self) -> sealedstruct::Result<()> {
        if **self < 0. {
            return sealedstruct::ValidationError::new(format!("{} is too small", self.0)).into();
        }

        if **self > 1. {
            return sealedstruct::ValidationError::new(format!("{} is too big", self.0)).into();
        }
        Ok(())
    }
}

impl Percentage {
    pub fn new(i: f64) -> sealedstruct::Result<Self> {
        PercentageRaw(i).seal()
    }
    pub fn max() -> Self {
        Percentage::new_unchecked(PercentageRaw(1.0))
    }
    pub fn min() -> Self {
        Percentage::new_unchecked(PercentageRaw(0.0))
    }

    pub fn fifty() -> Self {
        Percentage::new_unchecked(PercentageRaw(0.5))
    }
}

impl approx::AbsDiffEq for PercentageRaw {
    type Epsilon = f64;

    fn default_epsilon() -> Self::Epsilon {
        f64::EPSILON
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.0.abs_diff_eq(other, epsilon)
    }
}

impl PercentageRaw {
    pub fn new(i: f64) -> Self {
        Self(i)
    }

    pub fn value(&self) -> f64 {
        self.0
    }
}

impl Display for PercentageRaw {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("{}%", self.0 * 100.))
    }
}

impl Debug for Percentage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self, f)
    }
}

impl std::ops::Sub for PercentageRaw {
    type Output = PercentageRaw;

    fn sub(self, rhs: Self) -> Self::Output {
        PercentageRaw(self.0 - rhs.0)
    }
}

impl std::ops::Add for PercentageRaw {
    type Output = PercentageRaw;

    fn add(self, rhs: Self) -> Self::Output {
        PercentageRaw(self.0 + rhs.0)
    }
}

impl std::ops::Mul for PercentageRaw {
    type Output = PercentageRaw;

    fn mul(self, rhs: Self) -> Self::Output {
        PercentageRaw(self.0 * rhs.0)
    }
}

impl From<f64> for Percentage {
    fn from(i: f64) -> Self {
        Self(i.into())
    }
}
impl From<f64> for PercentageRaw {
    fn from(i: f64) -> Self {
        Self(i)
    }
}

impl Deref for PercentageRaw {
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
        assert!(PercentageRaw::new(-1.).seal().is_err());
        assert!(PercentageRaw::new(1.1).seal().is_err());
    }
}
