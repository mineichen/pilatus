use serde::{ser::SerializeMap, Deserialize, Serialize};

use super::ImageKey;
/// Can be used flattened in any Device-Params:
///
/// image_input:
///  - Unspecified or null: main image
///  - string: specific key is used
/// image_output:
///  - Unspecified: same as image_input,
///  - null: main image  
///  - string: specific key is used
#[derive(Default, Debug, PartialEq, Eq, Clone)]
pub struct ImageSelector {
    image_input: ImageKey,
    image_output: ImageSelectorOutput,
}

impl ImageSelector {
    pub fn new(input: ImageKey, output: ImageSelectorOutput) -> Self {
        Self {
            image_input: input,
            image_output: output,
        }
    }

    pub fn output(&self) -> &ImageKey {
        match &self.image_output {
            ImageSelectorOutput::SameAsInput => &self.image_input,
            ImageSelectorOutput::Selector(image_key) => &image_key,
        }
    }

    pub fn input(&self) -> &ImageKey {
        &self.image_input
    }
}

#[derive(Default, PartialEq, Eq, Debug, Clone)]
pub enum ImageSelectorOutput {
    #[default]
    SameAsInput,
    Selector(ImageKey),
}

impl Serialize for ImageSelector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let option_in = self.image_input.specific();
        let option_out = match &self.image_output {
            ImageSelectorOutput::SameAsInput => None,
            ImageSelectorOutput::Selector(s) => Some(s),
        };
        let mut map = serializer.serialize_map(Some(
            option_in.is_some() as usize + option_out.is_some() as usize,
        ))?;

        if let Some(x) = option_in {
            map.serialize_entry("image_input", x)?;
        }
        if let Some(x) = option_out {
            map.serialize_entry("image_output", x)?;
        }

        map.end()
    }
}
impl<'de> Deserialize<'de> for ImageSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(Visitor)
    }
}

struct Visitor;

impl<'de> serde::de::Visitor<'de> for Visitor {
    type Value = ImageSelector;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "Expected valid data")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut result = ImageSelector {
            image_input: Default::default(),
            image_output: Default::default(),
        };

        while let Some((k, v)) = map.next_entry::<&str, ImageKey>()? {
            match k {
                "image_input" => result.image_input = v,
                "image_output" => result.image_output = ImageSelectorOutput::Selector(v),
                _ => {
                    println!("Unknown: {k}:{v:?}")
                }
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Debug, Default)]
    #[serde(default)]
    struct Outer {
        other: i32,
        #[serde(flatten)]
        image_selector: ImageSelector,
    }

    #[test]
    fn deserialize_flattened_unspecified() {
        let value = serde_json::Value::Object(Default::default());
        assert_eq!(
            ImageSelector {
                image_input: Default::default(),
                image_output: ImageSelectorOutput::SameAsInput
            },
            Outer::deserialize(&value).unwrap().image_selector
        );
    }
    #[test]
    fn deserialize_flattened_null() {
        let value = serde_json::json!({
            "image_output": null,
            "other": 42
        });

        assert_eq!(
            ImageSelector {
                image_input: Default::default(),
                image_output: ImageSelectorOutput::Selector(ImageKey::unspecified())
            },
            Outer::deserialize(&value).unwrap().image_selector
        );
    }

    #[test]
    fn deserialize_flattened_explicit() {
        let value = serde_json::json!({
            "image_output": "foobar",
            "other": 42
        });
        let outer = Outer::deserialize(&value).unwrap();
        assert_eq!(
            ImageSelector {
                image_input: Default::default(),
                image_output: ImageSelectorOutput::Selector(ImageKey::try_from("foobar").unwrap())
            },
            outer.image_selector
        );
        assert_eq!(42, outer.other);
    }
    #[test]
    fn serialize_output_unspecified() {
        let before = ImageSelector::new(ImageKey::unspecified(), ImageSelectorOutput::SameAsInput);
        let as_str = serde_json::to_string(&before).unwrap();
        let after: ImageSelector = serde_json::from_str(&as_str).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn serialize_image_selector_output_null_input_unspecified() {
        let before = ImageSelector::new(
            ImageKey::unspecified(),
            ImageSelectorOutput::Selector(ImageKey::unspecified()),
        );
        let as_str = serde_json::to_string(&before).unwrap();
        let after: ImageSelector = serde_json::from_str(&as_str).unwrap();
        assert_eq!(before, after);
    }
    #[test]
    fn serialize_image_selector_output_null_input_specific() {
        let before = ImageSelector::new(
            ImageKey::try_from("blabla").unwrap(),
            ImageSelectorOutput::Selector(ImageKey::unspecified()),
        );
        let as_str = serde_json::to_string(&before).unwrap();
        let after: ImageSelector = serde_json::from_str(&as_str).unwrap();
        assert_eq!(before, after);
    }
    #[test]
    fn serialize_image_selector_output_specific_input_specific() {
        let before = ImageSelector::new(
            ImageKey::try_from("foo").unwrap(),
            ImageSelectorOutput::Selector(ImageKey::try_from("bar").unwrap()),
        );
        let as_str = serde_json::to_string(&before).unwrap();
        let after: ImageSelector = serde_json::from_str(&as_str).unwrap();
        assert_eq!(before, after);
    }
}
