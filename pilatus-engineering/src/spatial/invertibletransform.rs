use sealedstruct::ValidationError;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, sealedstruct::Seal)]
#[serde(deny_unknown_fields)]
pub struct InvertibleTransformRaw {
    pub m11: f64,
    pub m12: f64,
    pub m21: f64,
    pub m22: f64,
    pub m31: f64,
    pub m32: f64,
}

impl sealedstruct::Validator for InvertibleTransformRaw {
    fn check(&self) -> sealedstruct::Result<()> {
        (self.determinant() != 0.)
            .then_some(())
            .ok_or_else(|| ValidationError::new("Matrix is not invertible").into())
    }
}

impl InvertibleTransform {
    pub fn from_rotation_before_translation((x, y): (f64, f64), rotation: f64) -> Self {
        InvertibleTransform::new_unchecked(InvertibleTransformRaw {
            m11: rotation.cos(),
            m12: rotation.sin(),
            m21: -rotation.sin(),
            m22: rotation.cos(),
            m31: x,
            m32: y,
        })
    }
    pub fn identity() -> Self {
        InvertibleTransform::new_unchecked(InvertibleTransformRaw {
            m11: 1.,
            m12: 0.,
            m21: 0.,
            m22: 1.,
            m31: 0.,
            m32: 0.,
        })
    }
}

impl InvertibleTransformRaw {
    // Other parts cancel out with the last row being (0,0,1)
    fn determinant(&self) -> f64 {
        self.m11 * self.m22 - self.m21 * self.m12
    }
}
