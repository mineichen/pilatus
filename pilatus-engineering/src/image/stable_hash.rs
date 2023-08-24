use std::{
    borrow::Cow,
    fmt::{self, Debug, Display, Formatter},
    hash::{Hash, Hasher},
    num::NonZeroU64,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
#[repr(transparent)]
pub struct StableHash(NonZeroU64);

impl Debug for StableHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("StableHash")
            .field(&format_args!("0x{}", self.format_hex()))
            .finish()
    }
}

impl StableHash {
    pub fn from_hashable<T: Hash>(x: T) -> Self {
        let mut hasher = seahash::SeaHasher::new();
        x.hash(&mut hasher);

        Self(
            hasher
                .finish()
                .try_into()
                .unwrap_or(NonZeroU64::new(10101010110).unwrap()),
        )
    }
    pub fn format_hex(&self) -> impl Display + Debug {
        HexFormat(self.0)
    }
}
pub trait OptionalStableHash {
    fn get_change_state(&self, other: &Self) -> StableHashChangeState;
}

impl OptionalStableHash for Option<StableHash> {
    fn get_change_state(&self, other: &Self) -> StableHashChangeState {
        match (self, other) {
            (None, _) => StableHashChangeState::NeverHadHash,
            (Some(_), None) => StableHashChangeState::NewHashChangedToNone,
            (Some(x), Some(y)) => {
                if x == y {
                    StableHashChangeState::SameHash
                } else {
                    StableHashChangeState::NewHashDoesntMatch(*y)
                }
            }
        }
    }
}
#[derive(Debug, Serialize)]
pub enum StableHashChangeState {
    NeverHadHash,
    NewHashChangedToNone,
    NewHashDoesntMatch(StableHash),
    SameHash,
}

impl StableHashChangeState {
    pub fn get_warning(&self) -> Option<impl Debug + '_> {
        match self {
            StableHashChangeState::NeverHadHash => None,
            StableHashChangeState::NewHashChangedToNone => Some(self),
            StableHashChangeState::NewHashDoesntMatch(_) => Some(self),
            StableHashChangeState::SameHash => None,
        }
    }
}

// Serializes to HexString, because some environments (e.g. JS would interpret number as float and thus loose precision)
impl Serialize for StableHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        format_args!("{}", self.format_hex()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StableHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Cow::<str>::deserialize(deserializer)?;
        let num = u64::from_str_radix(&s, 16).map_err(<D::Error as serde::de::Error>::custom)?;
        let non_zero_num = NonZeroU64::new(num).ok_or_else(|| {
            <D::Error as serde::de::Error>::custom("'0' is not a valid number for maybe_hash")
        })?;

        Ok(Self(non_zero_num))
    }
}

struct HexFormat(NonZeroU64);

impl Debug for HexFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("HexFormat")
            .field(&format_args!("{}", &self.0))
            .finish()
    }
}

impl Display for HexFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut byte_writer = move |m| {
            let char = std::char::from_digit(m as u32, 16).unwrap();
            let mut dst = [0u8];
            f.write_str(char.encode_utf8(&mut dst)).unwrap();
        };
        for byte in self.0.get().to_le_bytes().into_iter().rev() {
            byte_writer(byte >> 4);
            byte_writer((byte << 4) >> 4);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_changes() {
        let none = Option::<StableHash>::None;
        assert!(none
            .get_change_state(&Some(StableHash(NonZeroU64::new(1).unwrap())))
            .get_warning()
            .is_none());
        assert!(Option::<StableHash>::None
            .get_change_state(&none)
            .get_warning()
            .is_none());

        assert!(Some(StableHash(NonZeroU64::new(1).unwrap()))
            .get_change_state(&none)
            .get_warning()
            .is_some());
        assert!(Some(StableHash(NonZeroU64::new(1).unwrap()))
            .get_change_state(&Some(StableHash(NonZeroU64::new(2).unwrap())))
            .get_warning()
            .is_some());
    }

    #[test]
    fn serialize_and_deserialize_some() {
        let x = StableHash(NonZeroU64::new(u64::MAX / 2).unwrap());
        let s = serde_json::to_string(&x).unwrap();
        let x_new = serde_json::from_str(dbg!(&s)).unwrap();
        assert_eq!(x, x_new);
    }

    #[test]
    fn serialize_and_deserialize_some_value() {
        let x = StableHash(NonZeroU64::new(10).unwrap());
        let s = serde_json::to_value(x).unwrap();
        let x_new = serde_json::from_value(s).unwrap();
        assert_eq!(x, x_new);
    }
}
