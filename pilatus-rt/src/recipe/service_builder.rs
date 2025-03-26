use std::{
    any::Any,
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::sync::RwLock;
use tracing::debug;

use super::InitRecipeListener;
use crate::recipe::RecipeServiceAccessor;
use pilatus::{Recipe, Recipes};

use super::actions::DeviceActions;

pub struct RecipeServiceBuilder {
    path: PathBuf,
    // Responsible for changes in the running configuration
    device_actions: Arc<dyn DeviceActions>,
    listeners: Vec<InitRecipeListener>,
    pub(super) change_strategies:
        HashMap<(&'static str, std::any::TypeId), Box<dyn Any + Send + Sync>>,
}
impl RecipeServiceBuilder {
    pub fn new(
        path: impl Into<PathBuf>,
        device_actions: Arc<dyn DeviceActions>,
    ) -> RecipeServiceBuilder {
        Self {
            path: path.into(),
            device_actions,
            listeners: Default::default(),
            change_strategies: Default::default(),
        }
    }

    pub fn replace_permissioner(mut self, permissioner: Arc<dyn DeviceActions>) -> Self {
        self.device_actions = permissioner;
        self
    }

    pub fn with_initializer(mut self, listener: InitRecipeListener) -> Self {
        self.listeners.push(listener);
        self
    }

    pub fn build(self) -> RecipeServiceAccessor {
        let mut path = self.path.join("recipes"); // /root/recipes
        for c in 1..100 {
            match Self::try_from_file_or_new(&path, &self.listeners) {
                Ok(recipes) => {
                    let (update_sender, _) = tokio::sync::broadcast::channel(10);
                    return RecipeServiceAccessor {
                        device_actions: self.device_actions,
                        path,
                        recipes: Arc::new(RwLock::new(recipes)),
                        listeners: self.listeners,
                        update_sender,
                        change_strategies: self.change_strategies,
                    };
                }
                Err(_) => {
                    path = self.path.join(format!("recipes_{}", c));
                }
            }
        }
        panic!("RecipeService cannot be started");
    }
    fn try_from_file_or_new(path: &Path, listeners: &[InitRecipeListener]) -> io::Result<Recipes> {
        let recipes: Recipes;
        let path = path.to_path_buf();
        std::fs::create_dir_all(&path)?; //create directory and all of its parent components if they are missing.

        let mut jpath = path; // root/recipes/
        jpath.push(super::RECIPES_FILE_NAME); // root/recipes/recipes.json

        if jpath.exists() {
            let file = std::fs::File::open(jpath.clone())?;
            recipes = Recipes::from_reader(file)?;
        } else {
            // create new recipes.json, as current path's folder is empty
            let mut r = Recipe::default();

            // add all default devices
            for listener in listeners.iter() {
                listener.call(&mut r);
            }

            recipes = Recipes::new_with_recipe(r);
            recipes.store_sync(jpath.clone())?;
            debug!("file {} created.", super::RECIPES_FILE_NAME);
        }

        Ok(recipes)
    }
}
