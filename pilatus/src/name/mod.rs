use std::fmt::{Debug, Display};

use sealedstruct::{ValidationError, ValidationErrors, ValidationResultExtensions};
use serde::{Deserialize, Serialize};

pub(crate) mod name_wrapper;

#[derive(
    PartialEq,
    Eq,
    Debug,
    PartialOrd,
    Ord,
    Clone,
    Hash,
    sealedstruct::SealSimple,
    Serialize,
    Deserialize,
)]
pub struct NameRaw(String);

impl std::str::FromStr for Name {
    type Err = ValidationErrors;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        NameRaw(s.into()).seal()
    }
}

impl Name {
    pub fn new(value: impl Into<String>) -> sealedstruct::Result<Self> {
        NameRaw(value.into()).seal()
    }

    pub fn suggest_unique(&self) -> impl Iterator<Item = Name> {
        let (base_number, base_name) = 'block: {
            if let Some((base, maybe_no)) = self.rsplit_once('_') {
                if let Ok(i) = maybe_no.parse() {
                    break 'block (i, base.to_string());
                }
            }
            (1, self.to_string())
        };

        (base_number..).map(move |n| NameWrapper(NameRaw(format!("{base_name}_{n}"))))
    }
}

impl Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl NameRaw {
    pub fn new(value: impl Into<String>) -> NameRaw {
        NameRaw(value.into())
    }
}

impl std::ops::Deref for NameRaw {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<NameRaw> for String {
    fn from(n: NameRaw) -> Self {
        n.0
    }
}

impl sealedstruct::Validator for NameRaw {
    fn check(&self) -> sealedstruct::Result<()> {
        let name = &self.0;
        let mut result: sealedstruct::Result<()> = Ok(());
        match name.len() {
            0 => {
                result = result.append_error(ValidationError::new("Empty name is not allowed"));
            }
            1..=30 => {}
            _ => {
                result =
                    result.append_error(ValidationError::new(format!("(len={}) > 30", name.len())));
            }
        }

        if name.starts_with(' ') {
            result = result.append_error(ValidationError::new(format!(
                "'{name}' is prefixed with whitespace"
            )));
        }

        if name.ends_with(' ') {
            result = result.append_error(ValidationError::new(format!(
                "'{name}' is suffixed with whitespace"
            )));
        }

        for c in name.chars() {
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | ' ' | '.' => continue,
                illegal_char => {
                    result = result.append_error(ValidationError::new(format!(
                        "invalid character {illegal_char}"
                    )));
                }
            }
        }
        result?;
        Ok(())
    }
}
