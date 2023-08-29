use std::sync::Arc;

use futures::{io::Cursor, StreamExt};
use pilatus::{
    visit_directory_files, DeviceConfig, ImportRecipeError, ImportRecipesOptions,
    IntoMergeStrategy, RecipeExporterTrait, RecipeServiceTrait,
};
use pilatus_rt::RecipeServiceFassade;

use crate::recipe::import::ZipReaderWrapper;

#[tokio::test]
async fn duplicate_self_allowed() {
    let (dir, rsb) = RecipeServiceFassade::create_temp_builder();
    let rs = Arc::new(rsb.build());
    let active_recipe_id = rs.get_active_id().await;

    let (export_recipe_id, _) = rs.duplicate_recipe(active_recipe_id).await.unwrap();

    let id = rs
        .add_device_to_recipe(export_recipe_id.clone(), DeviceConfig::mock(1i32))
        .await
        .unwrap();
    rs.create_device_file(id, "test.txt", b"content").await;
    let rs_clone = rs.clone();
    let export_recipe_id_clone = export_recipe_id.clone();
    let data = super::writer_into_vec_unchecked(move |w| {
        let rs = rs_clone;
        async move { rs.export(export_recipe_id_clone, w).await }
    })
    .await;

    let r = rs
        .create_importer()
        .import(
            &mut ZipReaderWrapper::new(Cursor::new(data.clone())),
            ImportRecipesOptions::default(),
        )
        .await;
    if let Err(ImportRecipeError::Conflicts(x, _, _)) = r {
        assert!(x.contains(&export_recipe_id));
    } else {
        panic!("Expected ExistsAlready-error, got {r:?}");
    }

    let r = rs
        .create_importer()
        .import(
            &mut ZipReaderWrapper::new(Cursor::new(data)),
            ImportRecipesOptions {
                merge_strategy: IntoMergeStrategy::Duplicate,
                is_dry_run: false,
            },
        )
        .await;
    if r.is_err() {
        panic!("Merge should work with duplicate strategy: {r:?}");
    }
    assert_eq!(
        2,
        visit_directory_files(dir.path())
            .filter_map(|x| async {
                let entry = x.ok()?;
                let filename = entry.file_name();
                filename.to_str().filter(|x| *x == "test.txt")?;
                let x = tokio::fs::read_to_string(entry.path()).await.ok()?;
                if x == "content" {
                    Some(())
                } else {
                    None
                }
            })
            .count()
            .await
    );
    assert_eq!(3, rs.state().await.recipes().iter_without_backup().count());
}
