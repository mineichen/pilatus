use std::{
    f64::consts::PI,
    fmt::{self, Debug, Formatter},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AngleError {
    #[error("Input '{0}' is not in range 0° .. 360°")]
    NotInRange(f64),
    #[error("Input is not a number")]
    InputIsNotANumber,
}

/// Holds angle in rad with range: 0..2PI
#[derive(Clone, Copy, Default, PartialEq, sealedstruct::IntoSealed)]
pub struct Angle(f64);

impl Serialize for Angle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Angle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        f64::deserialize(deserializer)
            .and_then(|v| Angle::try_from_rad(v).map_err(<D::Error as serde::de::Error>::custom))
    }
}

impl approx::AbsDiffEq for Angle {
    type Epsilon = f64;

    fn default_epsilon() -> Self::Epsilon {
        f64::EPSILON
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.0.abs_diff_eq(&other.0, epsilon)
    }
}

impl Debug for Angle {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AngleDeg: ")
            .field(&self.0.to_degrees())
            .finish()
    }
}
impl Angle {
    pub fn min() -> Self {
        Self(0.0)
    }

    pub fn max() -> Self {
        Self(PI * 2.)
    }
    ///Create Angle and return Err if value is not in range 0°..360°
    pub fn try_from_deg(i: f64) -> Result<Self, AngleError> {
        if i.is_nan() {
            Err(AngleError::InputIsNotANumber)
        } else if !(0.0..360.0).contains(&i) {
            Err(AngleError::NotInRange(i))
        } else {
            Ok(Self(i.to_radians()))
        }
    }

    ///Create Angle and wrap value to range 0°..360°
    pub fn try_from_deg_wrap(i: f64) -> Result<Self, AngleError> {
        if i.is_nan() {
            Err(AngleError::InputIsNotANumber)
        } else if i.is_infinite() {
            Err(AngleError::NotInRange(i))
        } else {
            Ok(Self(i.rem_euclid(360.).to_radians()))
        }
    }

    ///Create Angle and return Err if value is not in range 0.. 2PI°
    pub fn try_from_rad(i: f64) -> Result<Self, AngleError> {
        if i.is_nan() {
            Err(AngleError::InputIsNotANumber)
        } else if !(0.0..(2.0 * PI)).contains(&i) {
            Err(AngleError::NotInRange(i))
        } else {
            Ok(Self(i))
        }
    }
    ///Create Angle and wrap value to range 0°..360°
    pub fn try_from_rad_wrap(i: f64) -> Result<Self, AngleError> {
        if i.is_nan() {
            Err(AngleError::InputIsNotANumber)
        } else if i.is_infinite() {
            Err(AngleError::NotInRange(i))
        } else {
            Ok(Self(i.rem_euclid(2. * PI)))
        }
    }
    pub fn as_rad<T: FromAngle>(&self) -> T {
        T::convert_rad(*self)
    }
    pub fn as_mrad<T: FromAngle>(&self) -> T {
        T::convert_mrad(*self)
    }
    pub fn as_deg<T: FromAngle>(&self) -> T {
        T::convert_deg(*self)
    }
    pub fn as_mdeg<T: FromAngle>(&self) -> T {
        T::convert_mdeg(*self)
    }
    pub fn value_deg_str(&self) -> String {
        format!("{:.3}", self.as_deg::<f64>())
    }
}

pub trait FromAngle {
    fn convert_rad(a: Angle) -> Self;
    fn convert_mrad(a: Angle) -> Self;
    fn convert_deg(a: Angle) -> Self;
    fn convert_mdeg(a: Angle) -> Self;
}

impl FromAngle for f64 {
    fn convert_rad(a: Angle) -> Self {
        a.0
    }
    fn convert_mrad(a: Angle) -> Self {
        a.0 * 1000.0
    }
    fn convert_deg(a: Angle) -> Self {
        a.0 * 180. / PI
    }
    fn convert_mdeg(a: Angle) -> Self {
        a.0 * 180. / PI * 1000.0
    }
}

impl FromAngle for i64 {
    fn convert_rad(a: Angle) -> Self {
        a.0 as i64
    }
    fn convert_mrad(a: Angle) -> Self {
        (a.0 * 1000.0).round() as i64
    }
    fn convert_deg(a: Angle) -> Self {
        (a.0 * 180. / PI).round() as i64
    }
    fn convert_mdeg(a: Angle) -> Self {
        (a.0 * 180. / PI * 1000.0).round() as i64
    }
}

impl FromAngle for u32 {
    fn convert_rad(a: Angle) -> Self {
        a.0 as u32
    }
    fn convert_mrad(a: Angle) -> Self {
        (a.0 * 1000.0).round() as u32
    }
    fn convert_deg(a: Angle) -> Self {
        (a.0 * 180. / PI).round() as u32
    }
    fn convert_mdeg(a: Angle) -> Self {
        (a.0 * 180. / PI * 1000.0).round() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_invalid_angle() {
        serde_json::from_str::<Angle>("361").expect_err("Parse shouldn't work");
    }

    #[test]
    fn serialize_deserialize_angle() {
        let angle = Angle::try_from_deg(1.).unwrap();
        let anglestr = serde_json::to_string_pretty(&angle).unwrap();
        let read_angle = serde_json::from_str(&anglestr).unwrap();
        assert_eq!(angle, read_angle);
    }

    #[test]
    #[rustfmt::skip]
    fn angle_from_rad() {
        assert_eq!(Angle::try_from_deg(180.0).unwrap(), Angle::try_from_rad(PI).unwrap());
        Angle::try_from_rad(2. * PI).expect_err("Must be smaller than a full rotation");
        Angle::try_from_rad(f64::NAN).expect_err("Nan is not valid");
        Angle::try_from_rad(f64::INFINITY).expect_err("Infinity is not supported");
        Angle::try_from_rad(f64::NEG_INFINITY).expect_err("NegInfinity is not supported");
    }

    #[test]
    #[rustfmt::skip]
    fn angle_from_deg() {
        assert_eq!(Angle::try_from_deg(f64::NAN), Err(AngleError::InputIsNotANumber));
        assert_eq!(Angle::try_from_deg(f64::INFINITY), Err(AngleError::NotInRange(f64::INFINITY)));
        assert_eq!(Angle::try_from_deg(0.0).unwrap().as_deg::<f64>(), 0.0);
        assert_eq!(Angle::try_from_deg(360.0 - 0.00001).unwrap().as_deg::<f64>(),360.0 - 0.00001);
        assert_eq!(Angle::try_from_deg(360.0), Err(AngleError::NotInRange(360.0)));
        assert_eq!(Angle::try_from_deg(361.0), Err(AngleError::NotInRange(361.0)));
        assert_eq!(Angle::try_from_deg(-1.0), Err(AngleError::NotInRange(-1.0)));
    }

    #[test]
    #[rustfmt::skip]
    fn angle_from_deg_with_wrap() {
        assert_eq!(
            Angle::try_from_deg_wrap(f64::NAN),
            Err(AngleError::InputIsNotANumber)
        );
        assert_eq!(
            Angle::try_from_deg_wrap(f64::INFINITY),
            Err(AngleError::NotInRange(f64::INFINITY))
        );
        #[rustfmt::skip]
        assert_eq!(Angle::try_from_deg_wrap(0.0).unwrap().as_deg::<f64>(), 0.0);
        assert_eq!(Angle::try_from_deg_wrap(180.0).unwrap().as_deg::<f64>(), 180.0);
        assert_eq!(Angle::try_from_deg_wrap(-180.0).unwrap().as_deg::<f64>(), 180.0);
        assert_eq!(Angle::try_from_deg_wrap(360.0).unwrap().as_deg::<f64>(), 0.0);
        assert_eq!(Angle::try_from_deg_wrap(361.0).unwrap().as_deg::<f64>(), 1.0);
        assert_eq!(Angle::try_from_deg_wrap(-361.0).unwrap().as_deg::<f64>(), 359.0);
        assert_eq!(Angle::try_from_deg_wrap(359.0).unwrap().as_deg::<f64>(), 359.0);
        assert_eq!(Angle::try_from_deg_wrap(-359.0).unwrap().as_deg::<f64>(), 1.0);
        assert_eq!(Angle::try_from_deg_wrap(-359.0).unwrap().as_deg::<i64>(), 1);
        assert_eq!(Angle::try_from_deg_wrap(-180.0).unwrap().as_deg::<i64>(), 180);
        assert_eq!(Angle::try_from_deg_wrap(-361.3).unwrap().as_mdeg::<i64>(), 358700);
    }
}
