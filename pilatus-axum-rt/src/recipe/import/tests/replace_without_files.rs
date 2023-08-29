use std::sync::Arc;

use crate::recipe::import::ZipReaderWrapper;

use futures::io::Cursor;
use pilatus::{
    DeviceConfig, ImportRecipesOptions, IntoMergeStrategy, RecipeExporterTrait, RecipeServiceTrait,
};
use pilatus_rt::RecipeServiceFassade;

#[tokio::test]
async fn replace_without_files() {
    let (_dir, rsb) = RecipeServiceFassade::create_temp_builder();
    let rs = Arc::new(rsb.build());
    let active_recipe_id = rs.get_active_id().await;
    let (export_recipe_id, _) = rs.duplicate_recipe(active_recipe_id).await.unwrap();

    rs.add_device_to_recipe(export_recipe_id.clone(), DeviceConfig::mock(1i32))
        .await
        .unwrap();
    let rs_clone = rs.clone();
    let data = super::writer_into_vec_unchecked(move |w| {
        let rs = rs_clone;
        async move { rs.export(export_recipe_id, w).await }
    })
    .await;

    rs.create_importer()
        .import(
            &mut ZipReaderWrapper::new(Cursor::new(data)),
            ImportRecipesOptions {
                merge_strategy: IntoMergeStrategy::Replace,
                is_dry_run: false,
            },
        )
        .await
        .expect("Merge should work with replace strategy");

    assert_eq!(2, rs.state().await.recipes().iter_without_backup().count());
}
