use anyhow::anyhow;
use serde::de::DeserializeOwned;
use std::{
    collections::HashMap,
    io::Read,
    path::PathBuf,
    sync::{Arc, RwLock},
};

// Settings are received by topic... Entire topics will be saved
#[derive(Clone)]
pub struct Settings(Arc<(PathBuf, RwLock<HashMap<String, serde_json::Value>>)>);

impl Settings {
    pub fn new(filename: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let filename = filename.into();
        let mut data = Vec::new();

        let cache = match std::fs::File::open(&filename).and_then(|mut f| f.read_to_end(&mut data))
        {
            Ok(x) if x > 0 => serde_json::from_slice(&data[0..x]),
            _ => serde_json::from_str("{}"),
        };

        Ok(Self(Arc::new((filename, cache?))))
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> anyhow::Result<T> {
        let (_, cache) = self.0.as_ref();
        let data = cache.read().unwrap();
        let data = data.get(key).ok_or_else(|| anyhow!("Unknown key {key}"))?;
        T::deserialize(data).map_err(Into::into)
    }

    /// Inserts or updates the given key

    #[cfg(feature = "tokio")]
    pub async fn set<T: serde::Serialize>(&self, key: &str, value: T) -> anyhow::Result<()> {
        let (path, cache) = self.0.as_ref();
        let new_config: Vec<_> = {
            let json_value = serde_json::to_value(value)?;
            let mut lock = cache.write().unwrap();

            match lock.get_mut(key) {
                Some(x) => *x = json_value,
                None => {
                    lock.insert(key.to_string(), json_value);
                }
            };
            serde_json::to_vec_pretty(&*lock)?
        };
        tokio::fs::write(path, &new_config).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    #[cfg(feature = "tokio")]
    async fn read_write_settings() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let filepath = tmp.path().join("settings.json");
        let settings = crate::Settings::new(&filepath)?;
        let Err(_) = settings.get::<i32>("foo") else {
            panic!("Shouldn't have a value for 'foo'");
        };

        tokio::fs::File::create(&filepath).await?;

        let settings = crate::Settings::new(&filepath)?;
        settings.set("foo", 42).await.expect("Should have stored");

        assert_eq!(
            42,
            crate::Settings::new(filepath)?
                .get::<i32>("foo")
                .expect("Should have value for fo")
        );

        Ok(())
    }
}
