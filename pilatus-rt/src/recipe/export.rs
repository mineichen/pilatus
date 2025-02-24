use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use anyhow::anyhow;
use async_trait::async_trait;
use futures::{io::Cursor, pin_mut, StreamExt};
use pilatus::{EntryWriter, RecipeExporterTrait, RecipeId};
use tokio::fs;

use super::RecipeServiceFassade;

use super::RecipesExt;

#[async_trait]
impl RecipeExporterTrait for RecipeServiceFassade {
    async fn export<'a>(
        &self,
        recipe_id: RecipeId,
        mut writer: Box<dyn EntryWriter>,
    ) -> anyhow::Result<()> {
        let recipes_service = self.recipe_service_read().await;
        let recipes = &recipes_service.recipes;
        let recipe = recipes.get_with_id_or_error(&recipe_id)?;

        let recipe_string = serde_json::to_string_pretty(recipe)?;

        //write json data
        let filename = format!("{recipe_id}/recipe.json");

        writer
            .insert(filename, &mut Cursor::new(recipe_string.as_bytes()))
            .await?;

        let recipe_dir_path = self.recipe_dir_path();
        let recipe_id_str = recipe_id.to_string();
        let output_path_base = Path::new(&recipe_id_str);
        let mut used_variable_names = HashSet::new();
        for (&device_id, config) in recipe.devices.iter() {
            used_variable_names.extend(config.params.variables_names());
            let path = recipe_dir_path.join(device_id.to_string());
            if let Ok(meta) = fs::metadata(&path).await {
                if meta.is_dir() {
                    let files = super::visit_directory_files(path.clone());
                    pin_mut!(files);
                    while let Some(file) = files.next().await {
                        let filename_full_path = file?.path();
                        let entry_path = output_path_base
                            .join(filename_full_path.strip_prefix(recipe_dir_path)?)
                            .to_str()
                            .ok_or_else(|| anyhow!("invalid UTF-8"))?
                            .to_owned();
                        writer
                            .insert(
                                entry_path,
                                &mut tokio_util::compat::TokioAsyncReadCompatExt::compat(
                                    fs::File::open(filename_full_path).await?,
                                ),
                            )
                            .await?;
                    }
                }
            }
        }
        let variables = recipes.as_ref();
        let variable_map = used_variable_names
            .into_iter()
            .map(|x| match variables.resolve_key(&x) {
                Some(v) => Ok((x, v)),
                None => Err(anyhow!("Unknown variable '{}'", x)),
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        let mut cursor = Cursor::new(serde_json::to_vec(&variable_map)?);
        writer.insert("variables.json".into(), &mut cursor).await?;

        writer.close().await?;
        Ok(())
    }
}
