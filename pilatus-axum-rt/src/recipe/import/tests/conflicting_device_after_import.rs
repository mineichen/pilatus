use futures::io::Cursor;
use pilatus::{DeviceConfig, ImportRecipeError, ImportRecipesOptions, IntoMergeStrategy, RecipeId};
use pilatus_rt::RecipeServiceImpl;
use testresult::TestResult;

use crate::recipe::import::ZipReaderWrapper;

#[tokio::test]
async fn conflicting_device_after_import_replace() -> TestResult {
    import_recipe_with_different_id_but_including_existing_deviceid(IntoMergeStrategy::Replace)
        .await
}

#[tokio::test]
async fn conflicting_device_after_import_duplicate() -> TestResult {
    import_recipe_with_different_id_but_including_existing_deviceid(IntoMergeStrategy::Duplicate)
        .await
}
#[tokio::test]
async fn conflicting_device_after_import_unspecified() -> TestResult {
    import_recipe_with_different_id_but_including_existing_deviceid(IntoMergeStrategy::Unspecified)
        .await
}

async fn import_recipe_with_different_id_but_including_existing_deviceid(
    merge_strategy: IntoMergeStrategy,
) -> TestResult {
    let (_dir, rsb) = RecipeServiceImpl::create_temp_builder();
    let rs = rsb.build();
    let active_recipe_id = rs.get_active_id().await;
    let device_id = rs
        .add_device_to_active_recipe(DeviceConfig::mock(1i32))
        .await?;
    let import_recipe_id = RecipeId::default()
        .suggest_unique()
        .next()
        .ok_or("Suggestion iter is infinite")?;
    let zip_data = super::build_zip(
        import_recipe_id.clone(),
        device_id,
        DeviceConfig::mock(2i32),
        &[],
    )
    .await?;
    let import = rs
        .create_importer()
        .import(
            &mut ZipReaderWrapper::new(Cursor::new(zip_data)),
            ImportRecipesOptions {
                merge_strategy,
                is_dry_run: false,
            },
        )
        .await;
    match import {
        Err(ImportRecipeError::ExistingDeviceInOtherRecipe(
            error_device_id,
            error_recipe_id1,
            error_recipe_id2,
        )) => {
            assert_eq!(error_device_id, device_id);
            assert!(error_recipe_id1 == active_recipe_id || error_recipe_id2 == active_recipe_id);
            assert!(error_recipe_id1 == import_recipe_id || error_recipe_id2 == import_recipe_id);
        }
        x => {
            panic!("Unexpected response {x:?}");
        }
    }
    Ok(())
}
