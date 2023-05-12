use serde::{Deserialize, Serialize};

use pilatus::RelativeRange;

// Benefits in contrast to pixel-Bounds:
// - Can define meaningful default (0-1)
// - Can be validated independently of image-size
#[derive(Debug, PartialEq, Clone, Default, Serialize, Deserialize, sealedstruct::IntoSealed)]
#[serde(deny_unknown_fields)]
pub struct RelativeArea {
    pub column: RelativeRange,
    pub row: RelativeRange,
}

impl RelativeArea {
    // [col1, row1, col2, row2]
    // Todo: This calculation seems to be wrong.
    //   Mapping should be linear with rounding
    pub fn absolute(&self, dimensions: (u32, u32)) -> [u32; 4] {
        let (full_width, full_height) = (dimensions.0 as f64, dimensions.1 as f64);
        let col1 = **self.column.from * full_width;
        let row1 = **self.row.from * full_height;
        let col2 = **self.column.to * full_width;
        let row2 = **self.row.to * full_height;
        [
            col1 as u32,
            row1 as u32,
            (col2).min(full_width - 1.) as u32,
            (row2).min(full_height - 1.) as u32,
        ]
    }
    pub fn slice_horizontal(&self, at: &RelativeRange) -> RelativeAreaSliceHorizontal {
        let (left, center, right) = self.column.window_raw(at);
        let map = |column: RelativeRange| RelativeArea {
            column,
            row: self.row.clone(),
        };

        let left = left.map(map);
        let center = (map)(center);
        let right = right.map(map);

        RelativeAreaSliceHorizontal {
            left,
            center,
            right,
        }
    }
}

impl approx::AbsDiffEq for RelativeArea {
    type Epsilon = f64;

    fn default_epsilon() -> Self::Epsilon {
        f64::EPSILON
    }

    fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
        self.row.abs_diff_eq(&other.row, epsilon) && self.column.abs_diff_eq(&other.column, epsilon)
    }
}

pub struct RelativeAreaSliceHorizontal {
    pub left: Option<RelativeArea>,
    pub center: RelativeArea,
    pub right: Option<RelativeArea>,
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;

    #[test]
    fn absolute_region_max_returns_inbound_row_and_col() {
        let absolute = RelativeArea::default();
        let [col1, row1, col2, row2] = absolute.absolute((100, 100));
        assert_eq!(0, col1);
        assert_eq!(0, row1);
        assert_eq!(99, col2);
        assert_eq!(99, row2);
    }
    #[test]
    fn split_at_zero() {
        let raw = RelativeArea::default();
        let from_zero_range = RelativeRange::new(0., 0.5).unwrap();
        let left = raw.slice_horizontal(&from_zero_range).left;
        assert_eq!(None, left);
    }
    #[test]
    fn split_at_max() {
        let raw = RelativeArea::default();
        let to_max_range = RelativeRange::new(0.5, 1.).unwrap();
        let right = raw.slice_horizontal(&to_max_range).right;
        assert_eq!(None, right);
    }

    #[test]
    fn horizontal_window() {
        let raw = RelativeArea {
            column: RelativeRange::new(0.2, 1.0).unwrap(),
            row: RelativeRange::new(0.1, 0.9).unwrap(),
        };
        let range = RelativeRange::new(0.5, 0.75).unwrap();
        let RelativeAreaSliceHorizontal {
            left,
            center,
            right,
        } = raw.slice_horizontal(&range);

        let right = right.expect("Should have right part");
        let left = left.expect("Should have left part");

        assert_abs_diff_eq!(
            left,
            RelativeArea {
                column: RelativeRange::new(0.2, 0.6).unwrap(),
                ..raw.clone()
            }
        );
        assert_abs_diff_eq!(
            center,
            RelativeArea {
                column: RelativeRange::new(0.6, 0.8).unwrap(),
                ..raw.clone()
            }
        );

        assert_abs_diff_eq!(
            right,
            RelativeArea {
                column: RelativeRange::new(0.8, 1.0).unwrap(),
                ..raw
            },
        );
    }
}
