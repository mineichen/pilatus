use std::{
    collections::hash_map::Entry,
    ops::{Deref, DerefMut},
};

use anyhow::anyhow;
use serde::{de::DeserializeOwned, ser::SerializeStruct, Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::{
    UntypedDeviceParamsWithVariables, UpdateParamsMessageError, Variables, VariablesPatch,
};

#[derive(Debug)]
pub struct MaybeVar<T> {
    name: Option<String>,
    resolved: T,
}

impl<T> MaybeVar<T> {
    pub fn from_value(value: T) -> Self {
        Self {
            resolved: value,
            name: None,
        }
    }

    pub fn into_resolved(self) -> T {
        self.resolved
    }
    pub fn assign_variable(&mut self, var_name: impl Into<String>) {
        self.name = Some(var_name.into())
    }
}

impl<T: Serialize> Serialize for MaybeVar<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let Some(name) = &self.name {
            let mut s = serializer.serialize_struct("MaybeVar", 2)?;
            s.serialize_field("__var", &name)?;
            s.serialize_field("resolved", &self.resolved)?;
            s.end()
        } else {
            self.resolved.serialize(serializer)
        }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for MaybeVar<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum MaybevarDeserializeHelper<T> {
            Variable(MaybevarDeserializeHelperVariable<T>),
            Value(T),
        }
        #[derive(Deserialize)]
        struct MaybevarDeserializeHelperVariable<T> {
            __var: String,
            resolved: T,
        }

        match MaybevarDeserializeHelper::<T>::deserialize(deserializer)? {
            MaybevarDeserializeHelper::Value(v) => Ok(MaybeVar::from_value(v)),
            MaybevarDeserializeHelper::Variable(x) => Ok(MaybeVar {
                name: Some(x.__var),
                resolved: x.resolved,
            }),
        }
    }
}

impl<T> Deref for MaybeVar<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.resolved
    }
}

impl<T> DerefMut for MaybeVar<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.resolved
    }
}

/// Renders Variables into a
///
/// Because writing a custom Serializer is a lot of work, I choose to have more allocations and a separate loop to detect variable changes
/// The serde_json::value::Serializer cannot easily be wrapped, as it calls to_value() internally
impl Variables {
    pub fn resolve_var<T: DeserializeOwned>(
        &self,
        with_variables: &UntypedDeviceParamsWithVariables,
    ) -> Result<MaybeVar<T>, UpdateParamsMessageError> {
        let x = self.resolve_value(&with_variables.0, |var_name, value| {
            JsonValue::Object(
                [
                    ("__var".to_string(), JsonValue::String(var_name.into())),
                    ("resolved".to_string(), value),
                ]
                .into_iter()
                .collect(),
            )
        })?;
        serde_json::from_value(x).map_err(Into::into)
    }

    pub fn unresolve_var<T: RawVariable>(
        &self,
        x: &MaybeVar<T::Variable>,
    ) -> Result<(UntypedDeviceParamsWithVariables, Option<VariablesPatch>), anyhow::Error>
    where
        T::Variable: Serialize,
    {
        let json = serde_json::to_value(x)?;
        let mut patch = VariablesPatch::default();
        let v = self.remove_resolved(json, &mut patch)?;
        let params = UntypedDeviceParamsWithVariables::new(v);
        if patch.is_empty() {
            Ok((params, None))
        } else {
            Ok((params, Some(patch)))
        }
    }

    fn remove_resolved(
        &self,
        v: JsonValue,
        patch: &mut VariablesPatch,
    ) -> Result<JsonValue, anyhow::Error> {
        match v {
            x @ JsonValue::Null
            | x @ JsonValue::Bool(_)
            | x @ JsonValue::Number(_)
            | x @ JsonValue::String(_) => Ok(x),
            JsonValue::Array(a) => Ok(JsonValue::Array(
                a.into_iter()
                    .map(|item| self.remove_resolved(item, patch))
                    .collect::<Result<_, _>>()?,
            )),
            JsonValue::Object(mut o) => {
                let Some(JsonValue::String(var_name)) = o.get("__var") else {
                    return Ok(JsonValue::Object(o.into_iter().map(|(k,v)| {
                        self.remove_resolved(v, patch).map(|v| (k, v))
                    }).collect::<Result<_,_>>()?));
                };
                let var_name = var_name.to_string();
                let Some(x) = o.remove("resolved") else {
                    return Err(anyhow!("Expected value 'resolved' in {o:?}"));
                };
                let variable = crate::Variable::try_from(x)?;

                if self.mappings.get(&var_name) != Some(&variable) {
                    match patch.entry(var_name) {
                        Entry::Occupied(o) => {
                            if o.get() != &variable {
                                return Err(anyhow!(
                                    "Conflicting variables for {}: {:?} != {:?}",
                                    o.key(),
                                    o.get(),
                                    &variable
                                ));
                            }
                        }
                        Entry::Vacant(v) => {
                            v.insert(variable);
                        }
                    }
                }

                Ok(JsonValue::Object(o))
            }
        }
    }
}

pub trait RawVariable: Sized {
    type Variable: Into<Self>;
}

impl RawVariable for i32 {
    type Variable = i32;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::Variable;

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Foo {
        bar: Bar,
        foo: i32,
    }

    impl Default for Foo {
        fn default() -> Self {
            Self {
                bar: Default::default(),
                foo: 42,
            }
        }
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(default)]
    struct FooVar {
        foo: MaybeVar<<i32 as RawVariable>::Variable>,
        bar: MaybeVar<<Bar as RawVariable>::Variable>,
    }

    impl Default for FooVar {
        fn default() -> Self {
            Foo::default().into()
        }
    }

    impl From<Foo> for FooVar {
        fn from(value: Foo) -> Self {
            FooVar {
                #[allow(clippy::useless_conversion)]
                foo: MaybeVar::from_value(value.foo.into()),
                bar: MaybeVar::from_value(value.bar.into()),
            }
        }
    }

    impl From<FooVar> for Foo {
        fn from(value: FooVar) -> Self {
            Foo {
                #[allow(clippy::useless_conversion)]
                foo: value.foo.resolved.into(),
                bar: value.bar.resolved.into(),
            }
        }
    }

    impl RawVariable for Foo {
        type Variable = FooVar;
    }

    #[derive(serde::Serialize, serde::Deserialize, Default)]
    struct Bar {
        bar_inner: i32,
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(default)]
    struct BarVar {
        bar_inner: MaybeVar<<i32 as RawVariable>::Variable>,
    }

    impl Default for BarVar {
        fn default() -> Self {
            Bar::default().into()
        }
    }

    impl From<Bar> for BarVar {
        fn from(value: Bar) -> Self {
            BarVar {
                #[allow(clippy::useless_conversion)]
                bar_inner: MaybeVar::from_value(value.bar_inner.into()),
            }
        }
    }

    impl From<BarVar> for Bar {
        fn from(value: BarVar) -> Self {
            Bar {
                #[allow(clippy::useless_conversion)]
                bar_inner: value.bar_inner.resolved.into(),
            }
        }
    }

    impl RawVariable for Bar {
        type Variable = BarVar;
    }

    #[test]
    fn deserialize_maybevar_with_variable() {
        let original = json!({ "__var": "test", "resolved": 42});
        let v: MaybeVar<i32> = MaybeVar::deserialize(&original).unwrap();
        assert_eq!(42, *v);
        let serialized = serde_json::to_value(v).unwrap();
        assert_eq!(serialized, original);
    }

    #[test]
    fn deserialize_maybevar_without_variable() {
        let original = json!(42);
        let v: MaybeVar<i32> = MaybeVar::deserialize(&original).unwrap();
        assert_eq!(42, *v);
        let serialized = serde_json::to_value(v).unwrap();
        assert_eq!(serialized, original);
    }

    #[test]
    fn deserialize_and_serialize_without_variables() -> anyhow::Result<()> {
        let params =
            UntypedDeviceParamsWithVariables::new(json!({"foo": 1, "bar": {"bar_inner": 2 }}));
        let vars = Variables::default();
        let x: MaybeVar<FooVar> = vars.resolve_var(&params)?;
        assert_eq!(1, *x.foo);
        assert_eq!(2, *x.bar.bar_inner);
        let serialized = vars.unresolve_var::<Foo>(&x)?;

        assert_eq!((params, None), serialized);

        Ok(())
    }

    #[test]
    fn deserialize_and_serialize_with_unchanged_variables() -> anyhow::Result<()> {
        let params = UntypedDeviceParamsWithVariables::new(
            json! ({"foo": 1,"bar":{"bar_inner": {"__var": "testvar"}}}),
        );
        let vars = Variables::new(
            [("testvar".to_string(), Variable::from(42))]
                .into_iter()
                .collect(),
        );

        let foo: MaybeVar<FooVar> = vars.resolve_var(&params)?;
        assert_eq!(42, *foo.bar.bar_inner);
        let serialized = vars.unresolve_var::<Foo>(&foo)?;

        assert_eq!((params, None), serialized);

        Ok(())
    }

    #[test]
    fn deserialize_and_serialize_with_added_variable() -> anyhow::Result<()> {
        let params =
            UntypedDeviceParamsWithVariables::new(json! ({"foo": 1,"bar":{"bar_inner": 42}}));
        let vars = Variables::default();

        let mut foo: MaybeVar<FooVar> = vars.resolve_var(&params)?;
        foo.bar.bar_inner.assign_variable("my_var");
        let (new_serialized, new_variables) = match vars.unresolve_var::<Foo>(&foo) {
            Ok((serialized, Some(var_changes))) => {
                assert_eq!(1, var_changes.len());
                (serialized, vars.patch(var_changes))
            }
            e => panic!("Other error: {e:?}"),
        };

        let new_foo: MaybeVar<FooVar> = new_variables.resolve_var(&new_serialized)?;
        assert_eq!(*new_foo.bar.bar_inner, 42);

        Ok(())
    }
}
