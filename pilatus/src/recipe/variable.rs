use std::{
    borrow::Borrow,
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};

use anyhow::anyhow;
use serde::{
    de::{DeserializeOwned, Error},
    Deserialize, Serialize,
};
use serde_json::Value;

use crate::{recipe::UntypedDeviceParamsWithVariables, UpdateParamsMessageError};

pub(crate) const JSON_VAR_KEYWORD: &str = "__var";

mod maybe;

pub use maybe::*;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct VariableConflict {
    pub name: String,
    pub existing: Variable,
    pub imported: Variable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Variable(Value);
impl From<i32> for Variable {
    fn from(x: i32) -> Self {
        Self(serde_json::to_value(x).expect("Never fails with i32"))
    }
}

impl From<f64> for Variable {
    fn from(x: f64) -> Self {
        Self(serde_json::to_value(x).expect("Never fails with f64"))
    }
}

impl<'a> From<&'a str> for Variable {
    fn from(input: &'a str) -> Self {
        Self(serde_json::to_value(input).expect("Never fails with str"))
    }
}

impl TryFrom<serde_json::Value> for Variable {
    type Error = anyhow::Error;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        if value.is_number() || value.is_string() {
            Ok(Self(value))
        } else {
            Err(anyhow!("Just numbers and strings are supported"))
        }
    }
}

impl<'de> Deserialize<'de> for Variable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        v.try_into().map_err(<D::Error as serde::de::Error>::custom)
    }
}
pub type VariablesPatch = HashMap<String, Variable>;

#[derive(Debug, Clone, Default)]
pub struct Variables {
    mappings: Arc<HashMap<String, Variable>>,
}

impl Variables {
    fn new(mappings: HashMap<String, Variable>) -> Self {
        Self {
            mappings: Arc::new(mappings),
        }
    }

    pub fn add(&mut self, other: &Self) -> Vec<VariableConflict> {
        let mappings = self.borrow_mappings();
        other
            .mappings
            .iter()
            .filter_map(|(k, other_value)| match mappings.entry(k.into()) {
                Entry::Occupied(o) => (other_value != o.get()).then(|| VariableConflict {
                    name: o.key().into(),
                    existing: other_value.clone(),
                    imported: o.get().clone(),
                }),
                Entry::Vacant(x) => {
                    x.insert(other_value.clone());
                    None
                }
            })
            .collect()
    }

    fn borrow_mappings(&mut self) -> &mut HashMap<String, Variable> {
        if Arc::get_mut(&mut self.mappings).is_none() {
            self.mappings = Arc::new(HashMap::clone(&self.mappings));
        }
        Arc::get_mut(&mut self.mappings).unwrap()
    }
}

impl Serialize for Variables {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.mappings.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Variables {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        HashMap::deserialize(deserializer).map(Self::new)
    }
}

#[derive(Debug, Clone)]
pub struct UntypedDeviceParamsWithoutVariables(Value);

impl UntypedDeviceParamsWithoutVariables {
    pub fn params_as<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        T::deserialize(&self.0)
    }

    pub fn from_serializable<S: Serialize>(x: &S) -> serde_json::Result<Self> {
        Ok(Self(serde_json::to_value(x.borrow())?))
    }
}

impl Variables {
    pub fn patch(&self, patch: VariablesPatch) -> Self {
        Variables::new(
            self.mappings
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .chain(patch)
                .collect(),
        )
    }

    pub fn resolve(
        &self,
        with_variables: &UntypedDeviceParamsWithVariables,
    ) -> Result<UntypedDeviceParamsWithoutVariables, UpdateParamsMessageError> {
        self.resolve_value(&with_variables.0, |_, v| v)
            .map(UntypedDeviceParamsWithoutVariables)
    }

    pub fn resolve_key(&self, k: &str) -> Option<&Variable> {
        self.mappings.get(k)
    }

    fn resolve_value(
        &self,
        with_variables: &Value,
        generator: fn(&str, Value) -> Value,
    ) -> Result<Value, UpdateParamsMessageError> {
        match with_variables {
            x @ Value::Null | x @ Value::String(_) | x @ Value::Bool(_) | x @ Value::Number(_) => {
                Ok(x.clone())
            }

            Value::Array(x) => Ok(Value::Array(
                x.iter()
                    .map(|v| self.resolve_value(v, generator))
                    .collect::<Result<_, _>>()?,
            )),
            Value::Object(x) => {
                let mut iter = x.iter();
                if let Some((k, v)) = iter.next() {
                    if k == JSON_VAR_KEYWORD {
                        if let Some((k_other, _)) = iter.next() {
                            Err(UpdateParamsMessageError::InvalidFormat(
                                serde_json::Error::custom(format!(
                                    "Objects with __var mustn't contain anything else, got '{k_other}'"
                                )),
                            ))
                        } else if let Value::String(map_key) = v {
                            if let Some(new_value) = self.mappings.get(map_key) {
                                Ok((generator)(map_key, new_value.0.clone()))
                            } else {
                                Err(UpdateParamsMessageError::VariableError(format!(
                                    "Unknown Variable: {map_key}"
                                )))
                            }
                        } else {
                            Err(UpdateParamsMessageError::VariableError(
                                "Key starting with __var has to contain a string".into(),
                            ))
                        }
                    } else {
                        Ok(Value::Object(
                            x.iter()
                                .map(|(k, v)| {
                                    debug_assert_ne!(k, JSON_VAR_KEYWORD);
                                    Result::<_, UpdateParamsMessageError>::Ok((
                                        k.clone(),
                                        self.resolve_value(v, generator)?,
                                    ))
                                })
                                .collect::<Result<_, _>>()?,
                        ))
                    }
                } else {
                    Ok(Value::Object(Default::default()))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl AsRef<Variables> for Variables {
        fn as_ref(&self) -> &Variables {
            self
        }
    }

    impl AsMut<Variables> for Variables {
        fn as_mut(&mut self) -> &mut Variables {
            self
        }
    }

    #[tokio::test]
    async fn replace_valid_variable() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"_text": "value", "_number": 42, "placeholder": {"__var": "test"}}"#,
        )
        .unwrap();
        let store = Variables::new([("test".into(), (42).into())].into_iter().collect());

        #[derive(Deserialize)]
        struct Foo {
            _text: String,
            _number: i32,
            placeholder: i64,
        }
        let x = store
            .resolve(&UntypedDeviceParamsWithVariables(json))
            .unwrap();
        assert_eq!(42, x.params_as::<Foo>().unwrap().placeholder);
    }
}
