use serde::{Deserialize, Serialize};

use super::Percentage;

/// Non-Empty Range
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, sealedstruct::Seal)]
#[serde(deny_unknown_fields)]
pub struct RelativeRangeRaw {
    pub from: Percentage,
    pub to: Percentage,
}

impl Default for RelativeRangeRaw {
    fn default() -> Self {
        Self {
            from: Percentage::min(),
            to: Percentage::max(),
        }
    }
}

impl sealedstruct::Validator for RelativeRangeRaw {
    fn check(&self) -> sealedstruct::Result<()> {
        if self.from >= self.to {
            sealedstruct::ValidationError::on_fields(
                "from",
                ["to"],
                format!("{} must be smaller than {}", self.from, self.to),
            )
            .into()
        } else {
            Ok(())
        }
    }
}

impl approx::AbsDiffEq for RelativeRange {
    type Epsilon = f64;

    fn default_epsilon() -> Self::Epsilon {
        f64::EPSILON
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.from.abs_diff_eq(&other.from, epsilon) && self.to.abs_diff_eq(&other.to, epsilon)
    }
}

impl RelativeRange {
    pub fn window_raw(
        &self,
        range: &RelativeRange,
    ) -> (Option<RelativeRange>, RelativeRange, Option<RelativeRange>) {
        let width_percentage = *self.to - *self.from;
        let from_split_position = (*self.from + width_percentage * *range.from)
            .seal()
            .unwrap();
        let to_split_position = (*self.from + width_percentage * *range.to).seal().unwrap();

        let left = (**range.from > 0.).then(|| {
            RelativeRangeRaw {
                from: self.from,
                to: from_split_position,
            }
            .seal()
            .unwrap()
        });

        let center = RelativeRangeRaw {
            from: from_split_position,
            to: to_split_position,
        }
        .seal()
        .unwrap();

        let right = (**range.to < 1.).then(|| {
            RelativeRangeRaw {
                from: to_split_position,
                to: self.to,
            }
            .seal()
            .unwrap()
        });

        (left, center, right)
    }
}

impl RelativeRange {
    pub fn new(
        from: impl Into<Percentage>,
        to: impl Into<Percentage>,
    ) -> sealedstruct::Result<Self> {
        RelativeRangeRaw {
            from: from.into(),
            to: to.into(),
        }
        .seal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_ranges() {
        assert!(RelativeRange::new(0.5, 0.5).is_err());
        assert!(RelativeRange::new(0.6, 0.5).is_err());
    }
}
