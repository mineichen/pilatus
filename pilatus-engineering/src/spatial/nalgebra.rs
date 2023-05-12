use std::borrow::Borrow;

use nalgebra::{Matrix3, Matrix4};
use sealedstruct::ValidationErrors;

use crate::{
    InvertibleTransform, InvertibleTransform3d, InvertibleTransform3dRaw, InvertibleTransformRaw,
};

impl InvertibleTransform {
    pub fn to_nalgebra(&self) -> Matrix3<f64> {
        Matrix3::new(
            self.m11, self.m21, self.m31, self.m12, self.m22, self.m32, 0., 0., 1.,
        )
    }
}

impl InvertibleTransformRaw {
    pub fn from_nalgebra(t: impl Borrow<Matrix3<f64>>) -> InvertibleTransformRaw {
        let t = t.borrow();
        InvertibleTransformRaw {
            m11: t[(0, 0)],
            m21: t[(0, 1)],
            m31: t[(0, 2)],
            m12: t[(1, 0)],
            m22: t[(1, 1)],
            m32: t[(1, 2)],
        }
    }
}

impl InvertibleTransform3d {
    #[rustfmt::skip]
    pub fn to_nalgebra(&self) -> Matrix4<f64> {
        Matrix4::new(
            self.m11, self.m21, self.m31, self.m41,
            self.m12, self.m22, self.m32, self.m42,
            self.m13, self.m23, self.m33, self.m43,
            0.,    0.,    0.,    1.,
        )
    }

    pub fn from_nalgebra(
        t: impl Borrow<Matrix4<f64>>,
    ) -> Result<InvertibleTransform3d, ValidationErrors> {
        let t: &Matrix4<f64> = t.borrow();
        InvertibleTransform3dRaw {
            m11: t.m11,
            m21: t.m12,
            m31: t.m13,
            m41: t.m14,
            m12: t.m21,
            m22: t.m22,
            m32: t.m23,
            m42: t.m24,
            m13: t.m31,
            m23: t.m32,
            m33: t.m33,
            m43: t.m34,
        }
        .seal()
    }
}

#[cfg(test)]
mod tests {
    use nalgebra::Point2;

    use super::*;

    #[test]
    fn test_transform_back_and_forth() {
        let original = InvertibleTransformRaw {
            m11: 1.,
            m12: 0.,
            m21: 0.,
            m22: 1.,
            m31: 10.,
            m32: 12.,
        };
        let sealed = original.clone().seal().unwrap();
        let nalgebra = sealed.to_nalgebra();
        assert_eq!(
            Point2::new(0., 0.),
            nalgebra.transform_point(&Point2::new(-10., -12.))
        );
        let back = InvertibleTransformRaw::from_nalgebra(nalgebra);
        assert_eq!(original, back);
    }
}
