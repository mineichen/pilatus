use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    ffi::OsStr,
    io,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};

use anyhow::anyhow;
use async_trait::async_trait;
use futures::{
    io::{copy, Cursor},
    AsyncReadExt,
};
use pilatus::{
    EntryReader,
    ImportRecipeError::{self, InvalidFormat},
    ImportRecipesOptions, ImporterTrait, IntoMergeStrategy, IrreversibleError, Recipe, RecipeId,
    RecipeImporterTrait, Recipes, RelativeFilePath, Variables,
};
use tempfile::TempDir;
use tokio::{
    fs::{create_dir_all, File},
    task::spawn_blocking,
};
use tracing::{trace, warn};
use uuid::Uuid;

use self::merge_strategy::{MergeStrategy, MergeStrategyContext};
use super::RecipeServiceFassade;

mod merge_strategy;

#[must_use]
#[derive(Debug)]
pub struct Importer {
    service: Arc<RecipeServiceFassade>,
    recipes: HashMap<RecipeId, Recipe>,
    variables: Variables,
    tmp: TempDir,
}

#[async_trait]
impl ImporterTrait for Importer {
    async fn close_async(self: Box<Self>) -> io::Result<()> {
        tokio::task::spawn_blocking(move || self.tmp.close()).await?
    }

    async fn apply(
        self: Box<Self>,
        merge_strategy: IntoMergeStrategy,
    ) -> Result<(), ImportRecipeError> {
        match merge_strategy {
            IntoMergeStrategy::Unspecified => {
                self.apply_strategy(merge_strategy::Unspecified).await
            }
            IntoMergeStrategy::Duplicate => {
                self.apply_strategy(merge_strategy::Duplicate::new()).await
            }
            IntoMergeStrategy::Replace => self.apply_strategy(merge_strategy::Replace::new()).await,
        }
    }
}

impl Importer {
    async fn apply_strategy(
        self: Box<Self>,
        mut strategy: impl MergeStrategy,
    ) -> Result<(), ImportRecipeError> {
        let service = self.service.clone();
        let mut recipes_lock = service.recipe_service_write().await;

        let (active_id, _) = recipes_lock.recipes.get_active();
        let mut recipes_copy = recipes_lock.recipes.clone();

        let mut errors = HashSet::new();

        let variable_conflicts = recipes_copy.as_mut().add(&self.variables);

        for (recipe_id, recipe) in self.recipes.iter() {
            if *recipe_id == active_id {
                return Err(ImportRecipeError::ContainsActiveRecipe);
            }
            if strategy
                .handle_json(
                    MergeStrategyContext {
                        recipes_copy: &mut recipes_copy,
                        device_actions: self.service.recipe_service().device_actions.as_ref(),
                    },
                    recipe_id.clone(),
                    recipe.clone(),
                )
                .await
                .is_err()
            {
                errors.insert(recipe_id.clone());
            }
        }

        if !errors.is_empty() || !variable_conflicts.is_empty() {
            return Err(ImportRecipeError::Conflicts(
                errors,
                variable_conflicts,
                self,
            ));
        }
        if let Err(e) = error_for_duplicate_device(&recipes_copy) {
            self.close_async().await?;
            return Err(e);
        }

        let finalize = async {
            let recipe_path = self.service.recipe_dir_path();
            strategy.finalize(recipe_path, self.tmp.path()).await?;

            pilatus::clone_directory_deep(self.tmp.path(), recipe_path).await?;
            *recipes_lock.recipes = recipes_copy;
            recipes_lock.commit(Uuid::new_v4()).await?;
            Result::<_, IrreversibleError>::Ok(())
        }
        .await;
        if let Err(e) = finalize {
            self.close_async().await?;
            return Err(e.into());
        }
        Ok(())
    }
}

type ImportResult = Result<ImporterData, ImportRecipeError>;
type ImporterData = (HashMap<RecipeId, Recipe>, Variables);

pub struct RecipeImporterImpl(pub Arc<RecipeServiceFassade>);

#[async_trait]
impl RecipeImporterTrait for RecipeImporterImpl {
    async fn import(
        &self,
        reader: &mut dyn EntryReader,
        options: ImportRecipesOptions,
    ) -> Result<(), ImportRecipeError> {
        let tmp = spawn_blocking(tempfile::tempdir)
            .await
            .map_err(|e| ImportRecipeError::Io(e.into()))??;
        let path = tmp.path().into();
        let recipes = self.0.import_into_path(reader, path).await;

        match recipes {
            Ok((recipes, variables)) => {
                Box::new(Importer {
                    service: self.0.clone(),
                    recipes,
                    variables,
                    tmp,
                })
                .apply(options.merge_strategy)
                .await
            }
            Err(x) => {
                spawn_blocking(|| tmp.close())
                    .await
                    .map_err(|e| ImportRecipeError::Io(e.into()))??;
                Err(x)
            }
        }
    }
}
impl RecipeServiceFassade {
    async fn import_into_path(&self, r: &mut dyn EntryReader, root: PathBuf) -> ImportResult {
        let mut data = Vec::new();
        let mut recipes = HashMap::new();
        let mut variables: Result<Variables, _> =
            Err(InvalidFormat(anyhow!("Variables.json not found")));
        const MAX_JSON_FILE_SIZE_LIMIT: usize = 100 * 1024 * 1024;
        trace!("Import into path {root:?}");
        debug_assert!(root.exists(), "Expected {root:?} to exist");

        while let Some(entry) = r.next().await {
            let mut entry = entry.map_err(|e| ImportRecipeError::InvalidFormat(e.into()))?;
            if entry.filename == "variables.json" {
                data.clear();
                let consumed_bytes = entry
                    .reader
                    .take(MAX_JSON_FILE_SIZE_LIMIT as u64)
                    .read_to_end(&mut data)
                    .await?;
                if consumed_bytes == MAX_JSON_FILE_SIZE_LIMIT {
                    warn!("Variables are too big. Max is: {MAX_JSON_FILE_SIZE_LIMIT}");
                    return Err(InvalidFormat(anyhow!(
                        "Variables are too big. Max is: {MAX_JSON_FILE_SIZE_LIMIT}"
                    )));
                }

                variables = serde_json::from_slice(&data).map_err(|e| InvalidFormat(e.into()));
                continue;
            }
            let filename = PathBuf::from(entry.filename);
            let mut filename_iter = filename.iter().filter_map(OsStr::to_str);
            let recipe_id = filename_iter.next().ok_or_else(|| {
                InvalidFormat(anyhow!(
                    "All files except variables.json must be in a subfolder. Got: {filename:?}"
                ))
            })?;

            let recipe_id = recipe_id
                .parse::<RecipeId>()
                .map_err(|e| InvalidFormat(e.into()))?;

            match filename_iter.next() {
                Some("recipe.json") if filename_iter.next().is_none() => {
                    let mut cursor = Cursor::new(&mut data);
                    copy(
                        &mut entry.reader.take(MAX_JSON_FILE_SIZE_LIMIT as _),
                        &mut cursor,
                    )
                    .await?;

                    let data = cursor.into_inner();
                    if data.len() == MAX_JSON_FILE_SIZE_LIMIT {
                        return Err(InvalidFormat(anyhow!(
                            "recipe.json of {recipe_id} is too big. Max is: {MAX_JSON_FILE_SIZE_LIMIT}"
                        )));
                    }
                    let recipe: Recipe = serde_json::from_slice(data.as_slice())
                        .map_err(|e| InvalidFormat(e.into()))?;

                    recipes.insert(recipe_id, recipe);
                }
                Some(device_id) => {
                    Uuid::from_str(device_id).map_err(|e| InvalidFormat(e.into()))?;
                    let mut path = root.join(device_id);
                    let relative = RelativeFilePath::new(filename_iter.collect::<PathBuf>())
                        .map_err(|e| InvalidFormat(e.into()))?;
                    path.push(relative.as_path());

                    trace!("Create dir all {path:?}");
                    create_dir_all(&path.parent().expect("Must exist")).await?;
                    copy(
                        &mut entry.reader,
                        &mut tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(
                            File::create(&path).await?,
                        ),
                    )
                    .await?;
                    trace!("Copied file to {path:?}");
                }
                _ => {
                    return Err(InvalidFormat(anyhow!(
                        "all files except recipe.json in {recipe_id} must be in a subfolder. Got {filename:?}"
                    )));
                }
            };
        }

        Ok((recipes, variables?))
    }
}

#[allow(clippy::result_large_err)]
fn error_for_duplicate_device(r: &Recipes) -> Result<(), ImportRecipeError> {
    let mut known = HashMap::new();

    for (did, rid) in r.recipeid_per_deviceid() {
        match known.entry(did) {
            Entry::Occupied(x) => {
                return Err(ImportRecipeError::ExistingDeviceInOtherRecipe(
                    did,
                    rid,
                    x.remove(),
                ));
            }
            Entry::Vacant(x) => {
                x.insert(rid);
            }
        }
    }

    Ok(())
}
