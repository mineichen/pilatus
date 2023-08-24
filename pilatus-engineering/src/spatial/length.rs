use std::fmt::{self, Debug, Display, Formatter};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[repr(transparent)]
#[serde(deny_unknown_fields)]
pub struct Length(i64); //stored in micro meters

impl Debug for Length {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Length").field(&self.0).finish()
    }
}

impl Display for Length {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Length {
    pub fn from_m(i: f64) -> Self {
        Self((i * 1000000.) as i64)
    }

    pub fn from_mm(i: f64) -> Self {
        Self((i * 1000.) as i64)
    }

    pub fn from_um(i: i64) -> Self {
        Self(i)
    }

    pub fn um<T: FromLength>(&self) -> T {
        T::convert_micro(*self)
    }

    pub fn mm<T: FromLength>(&self) -> T {
        T::convert_millis(*self)
    }
    pub fn m<T: FromLength>(&self) -> T {
        T::convert_meters(*self)
    }
    pub fn mm_str(&self) -> String {
        format!("{:.3}", self.mm::<f64>())
    }
}

pub trait FromLength {
    fn convert_micro(l: Length) -> Self;
    fn convert_millis(l: Length) -> Self;
    fn convert_meters(l: Length) -> Self;
}

impl FromLength for f64 {
    fn convert_micro(l: Length) -> Self {
        l.0 as f64
    }
    fn convert_millis(l: Length) -> Self {
        (l.0 as f64) / 1000.
    }
    fn convert_meters(l: Length) -> Self {
        (l.0 as f64) / 1000000.
    }
}

impl FromLength for i64 {
    fn convert_micro(l: Length) -> Self {
        l.0
    }
    fn convert_millis(l: Length) -> Self {
        l.0 / 1000
    }
    fn convert_meters(l: Length) -> Self {
        l.0 / 1000000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_from_micro() {
        assert_eq!(Length::from_um(1), Length::from_m(0.000001));
    }

    #[test]
    fn length_from_mm() {
        assert_eq!(Length::from_mm(1.), Length::from_um(1000));
    }

    #[test]
    fn length_from_m() {
        assert_eq!(Length::from_m(1.), Length::from_um(1000000));
    }

    #[test]
    fn length_value_m() {
        assert_eq!(0.000001f64, Length::from_um(1).m::<f64>());
    }

    #[test]
    fn length_value_mm() {
        assert_eq!(0.001f64, Length::from_um(1).mm::<f64>());
    }

    #[test]
    fn length_value_mm_str() {
        assert_eq!("0.001".to_string(), Length::from_um(1).mm_str());
    }
}
