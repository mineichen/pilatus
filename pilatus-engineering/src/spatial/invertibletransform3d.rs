use sealedstruct::ValidationError;
use serde::{Deserialize, Serialize};

use crate::Frame;
use crate::{Angle, Length, XYZ, ZYX};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, sealedstruct::SealSimple)]
#[serde(deny_unknown_fields)]
pub struct InvertibleTransform3dRaw {
    pub m11: f64,
    pub m12: f64,
    pub m13: f64,
    pub m21: f64,
    pub m22: f64,
    pub m23: f64,
    pub m31: f64,
    pub m32: f64,
    pub m33: f64,
    pub m41: f64,
    pub m42: f64,
    pub m43: f64,
}

impl sealedstruct::Validator for InvertibleTransform3dRaw {
    fn check(&self) -> sealedstruct::Result<()> {
        let det = InvertibleTransform3d::new_unchecked(self.clone()).determinant();
        (det != 0.)
            .then_some(())
            .ok_or_else(|| ValidationError::new("Matrix is not invertible").into())
    }
}

impl InvertibleTransform3d {
    pub fn to_frame<T: private::AngleExtractor>(&self) -> Frame<T> {
        let (ar, br, cr) = T::get_angle(self);
        Frame::new(
            Length::from_m(self.m41),
            Length::from_m(self.m42),
            Length::from_m(self.m43),
            Angle::try_from_rad_wrap(ar).expect("calculated angles are valid"),
            Angle::try_from_rad_wrap(br).expect("calculated angles are valid"),
            Angle::try_from_rad_wrap(cr).expect("calculated angles are valid"),
        )
    }
    fn determinant(&self) -> f64 {
        self.m11 * self.m22 * self.m33
            - self.m11 * self.m32 * self.m23
            - self.m21 * self.m12 * self.m33
            + self.m31 * self.m12 * self.m23
            + self.m21 * self.m32 * self.m13
            - self.m31 * self.m22 * self.m13
    }
}

mod private {
    use super::InvertibleTransform3d;

    pub trait AngleExtractor {
        fn get_angle(m: &InvertibleTransform3d) -> (f64, f64, f64);
    }
}

impl private::AngleExtractor for XYZ {
    //euler angles from rotation matrix: https://threejs.org/
    fn get_angle(m: &InvertibleTransform3d) -> (f64, f64, f64) {
        let ar;
        let br = m.m31.clamp(-1., 1.).asin();
        let cr;
        if m.m31.abs() < 0.99999 {
            ar = (-m.m32).atan2(m.m33);
            cr = (-m.m21).atan2(m.m11);
        } else {
            ar = m.m23.atan2(m.m22);
            cr = 0.;
        }
        (ar, br, cr)
    }
}

impl private::AngleExtractor for ZYX {
    //euler angles from rotation matrix: https://threejs.org/
    fn get_angle(m: &InvertibleTransform3d) -> (f64, f64, f64) {
        let ar;
        let br = (-m.m13.clamp(-1., 1.)).asin();
        let cr;
        if m.m13.abs() < 0.99999 {
            ar = m.m23.atan2(m.m33);
            cr = m.m12.atan2(m.m11);
        } else {
            ar = 0.;
            cr = (-m.m21).atan2(m.m22);
        }
        (ar, br, cr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[rustfmt::skip]
    fn determinant_3d_equals_to_nalgebra() {
        let nalgebra = nalgebra::Matrix4::new(
            1.,  2.,   3., -4.,
            5., -6.,   7.,  8.,
            9., 10., -11., 12.,
             0., 0.,   0.,  1.,
        );
        let invertible = InvertibleTransform3dRaw {
            m11: 1.,
            m12: 5.,
            m13: 9.,
            m21: 2.,
            m22: -6.,
            m23: 10.,
            m31: 3.,
            m32: 7.,
            m33: -11.,
            m41: -4.,
            m42: 8.,
            m43: 12.,
        }
        .seal()
        .unwrap();
        assert_eq!(nalgebra.determinant(), invertible.determinant());
    }

    #[test]
    fn test_transform_x() {
        //matrix composed from rotations by rx:90°, (XYZ)
        let calculated_frame = InvertibleTransform3dRaw {
            m11: 1.,
            m12: 0.,
            m13: 0.,
            m21: 0.,
            m22: 0., //cos(x)
            m23: 1., //sin(x)
            m31: 0.,
            m32: -1., //-sin(x)
            m33: 0.,  //cos(x)
            m41: 0.01,
            m42: 0.02,
            m43: 0.1,
        }
        .seal()
        .unwrap()
        .to_frame::<XYZ>();

        let expected_frame = Frame::<XYZ>::new(
            Length::from_mm(10.),
            Length::from_mm(20.),
            Length::from_mm(100.),
            Angle::try_from_deg(90.).unwrap(),
            Angle::try_from_deg(0.).unwrap(),
            Angle::try_from_deg(0.).unwrap(),
        );
        assert_eq!(calculated_frame, expected_frame);
    }

    #[test]
    fn test_transform_y() {
        //matrix composed from rotations by ry:90°, (XYZ)
        let calculated_frame = InvertibleTransform3dRaw {
            m11: 0.,
            m12: 0.,
            m13: -1.,
            m21: 0.,
            m22: 1.,
            m23: 0.,
            m31: 1.,
            m32: 0.,
            m33: 0.,
            m41: 0.01,
            m42: 0.02,
            m43: 0.1,
        }
        .seal()
        .unwrap()
        .to_frame::<XYZ>();

        let expected_frame = Frame::<XYZ>::new(
            Length::from_mm(10.),
            Length::from_mm(20.),
            Length::from_mm(100.),
            Angle::try_from_deg(0.).unwrap(),
            Angle::try_from_deg(90.).unwrap(),
            Angle::try_from_deg(0.).unwrap(),
        );
        assert_eq!(calculated_frame, expected_frame);
    }

    #[test]
    fn test_transform_z() {
        //matrix composed from rotations by rz:90°, (XYZ)
        let calculated_frame = InvertibleTransform3dRaw {
            m11: 0.,
            m12: 1.,
            m13: 0.,
            m21: -1.,
            m22: 0.,
            m23: 0.,
            m31: 0.,
            m32: 0.,
            m33: 1.,
            m41: 0.01,
            m42: 0.02,
            m43: 0.1,
        }
        .seal()
        .unwrap()
        .to_frame::<XYZ>();

        let expected_frame = Frame::<XYZ>::new(
            Length::from_mm(10.),
            Length::from_mm(20.),
            Length::from_mm(100.),
            Angle::try_from_deg(0.).unwrap(),
            Angle::try_from_deg(0.).unwrap(),
            Angle::try_from_deg(90.).unwrap(),
        );
        assert_eq!(calculated_frame, expected_frame);
    }

    #[test]
    fn test_transform_xy() {
        //matrix composed from rotations by  rx:180°, ry:90°, (XYZ)
        //tool used: https://www.andre-gaschler.com/rotationconverter/ 21.10.2022
        let calculated_frame = InvertibleTransform3dRaw {
            m11: 0.,
            m12: 0.,
            m13: 1.,
            m21: 0.,
            m22: -1.,
            m23: 0.,
            m31: 1.,
            m32: 0.,
            m33: 0.,
            m41: 0.01,
            m42: 0.02,
            m43: 0.1,
        }
        .seal()
        .unwrap()
        .to_frame::<XYZ>();

        let expected_frame = Frame::<XYZ>::new(
            Length::from_mm(10.),
            Length::from_mm(20.),
            Length::from_mm(100.),
            Angle::try_from_deg(180.).unwrap(),
            Angle::try_from_deg(90.).unwrap(),
            Angle::try_from_deg(0.).unwrap(),
        );
        assert_eq!(calculated_frame, expected_frame);
    }

    #[test]
    fn test_transform_zy() {
        //matrix composed from rotations by  rz:-90°, ry:90°, (ZYX)
        //tool used: https://www.andre-gaschler.com/rotationconverter/ 21.10.2022
        let calculated_frame = InvertibleTransform3dRaw {
            m11: 0.,
            m12: 0.,
            m13: -1.,
            m21: 1.,
            m22: 0.,
            m23: 0.,
            m31: 0.,
            m32: -1.,
            m33: 0.,
            m41: 0.01,
            m42: 0.02,
            m43: 0.1,
        }
        .seal()
        .unwrap()
        .to_frame::<ZYX>();

        let expected_frame = Frame::<ZYX>::new(
            Length::from_mm(10.),
            Length::from_mm(20.),
            Length::from_mm(100.),
            Angle::try_from_deg(0.).unwrap(),
            Angle::try_from_deg(90.).unwrap(),
            Angle::try_from_deg(270.).unwrap(),
        );
        assert_eq!(calculated_frame, expected_frame);
    }

    #[test]
    fn test_transform_xyz() {
        //matrix composed from rotations by rx:12.3°, ry:-0.3°, rz:180° (XYZ)
        //tool used: https://www.andre-gaschler.com/rotationconverter/ 21.10.2022
        let calculated_frame = InvertibleTransform3dRaw {
            m11: -0.9999863,
            m12: 0.0011154,
            m13: -0.0051158,
            m21: -0.,
            m22: -0.9770456,
            m23: -0.2130304,
            m31: -0.0052360,
            m32: -0.2130275,
            m33: 0.9770322,
            m41: 0.01,
            m42: 0.02,
            m43: 0.1,
        }
        .seal()
        .unwrap()
        .to_frame::<XYZ>();

        let expected_frame = Frame::<XYZ>::new(
            Length::from_mm(10.),
            Length::from_mm(20.),
            Length::from_mm(100.),
            Angle::try_from_deg(12.3).unwrap(),
            Angle::try_from_deg(359.7).unwrap(),
            Angle::try_from_deg(180.0).unwrap(),
        );
        assert!(approx::abs_diff_eq!(
            calculated_frame,
            expected_frame,
            epsilon = 0.00001
        ));
    }
}
