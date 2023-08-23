use std::num::NonZeroU32;

use pilatus::Percentage;

use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, sealedstruct::IntoSealed, Deserialize, Serialize, Clone)]
pub struct RelativePolygon(pub Vec<(Percentage, Percentage)>);

impl RelativePolygon {
    pub fn absolute(&self, dimensions: (NonZeroU32, NonZeroU32)) -> Vec<(u32, u32)> {
        let x_dist = (dimensions.0.get() - 1) as f64;
        let y_dist = (dimensions.1.get() - 1) as f64;

        self.0
            .iter()
            .map(|(col_rel, row_rel)| {
                (
                    (col_rel.into_inner().value() * x_dist + 0.5) as u32,
                    (row_rel.into_inner().value() * y_dist + 0.5) as u32,
                )
            })
            .collect::<Vec<_>>()
    }
}

impl Default for RelativePolygon {
    fn default() -> Self {
        Self(vec![
            (Percentage::min(), Percentage::min()),
            (Percentage::max(), Percentage::min()),
            (Percentage::max(), Percentage::max()),
            (Percentage::min(), Percentage::max()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculate_absolute_region() {
        let area = RelativePolygon::default();
        let size = 100.try_into().unwrap();
        assert_eq!(
            vec![(0, 0), (99, 0), (99, 99), (0, 99)],
            area.absolute((size, size))
        );
    }

    #[test]
    fn absolute_around_tippingpoint() {
        let points = vec![
            (
                Percentage::new(0.124999).unwrap(),
                Percentage::new(0.125001).unwrap(),
            ),
            (
                Percentage::new(0.624999).unwrap(),
                Percentage::new(0.125001).unwrap(),
            ),
            (
                Percentage::new(0.624999).unwrap(),
                Percentage::new(0.625001).unwrap(),
            ),
            (
                Percentage::new(0.124999).unwrap(),
                Percentage::new(0.625001).unwrap(),
            ),
        ];
        let area = RelativePolygon(points);
        let size = 5.try_into().unwrap();

        assert_eq!(
            vec![(0, 1), (2, 1), (2, 3), (0, 3)],
            area.absolute((size, size))
        );
    }
}
