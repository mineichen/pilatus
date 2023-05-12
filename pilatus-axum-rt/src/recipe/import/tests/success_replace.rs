use futures::{io::Cursor, StreamExt};
use pilatus::{
    visit_directory_files, DeviceConfig, ImportRecipeError, ImportRecipesOptions,
    IntoMergeStrategy, RecipeServiceTrait, TransactionOptions,
};
use pilatus_rt::RecipeServiceImpl;

use crate::recipe::import::{tests::build_zip, ZipReaderWrapper};

#[tokio::test]
async fn same_device_replaces_with_all_files() {
    let (dir, rsb) = RecipeServiceImpl::create_temp_builder();
    let rs = rsb.build();
    let active_recipe_id = rs.get_active_id().await;

    let (export_recipe_id, _) = rs
        .clone_recipe(active_recipe_id, Default::default())
        .await
        .unwrap();

    let device_id = rs
        .add_device_to_recipe(
            export_recipe_id.clone(),
            DeviceConfig::mock(1i32),
            TransactionOptions::default(),
        )
        .await
        .unwrap();
    rs.create_device_file(device_id, "testdir/test.txt", b"foo")
        .await;
    rs.create_device_file(device_id, "foo.txt", b"bar").await;

    let conflicting_import_result = RecipeServiceImpl::create_importer(rs)
        .import(
            &mut ZipReaderWrapper::new(Cursor::new(
                build_zip(
                    export_recipe_id,
                    device_id,
                    DeviceConfig::mock(2i32),
                    &[("testdir/test.txt", "bar"), ("bar.txt", "bar")],
                )
                .await,
            )),
            ImportRecipesOptions {
                merge_strategy: IntoMergeStrategy::Replace,
                is_dry_run: false,
            },
        )
        .await;
    if let Err(ImportRecipeError::Conflicts(_, _, imp)) = conflicting_import_result {
        imp.close_async().await.unwrap();
    }

    assert_eq!(
        2,
        visit_directory_files(dir.path())
            .filter_map(|x| async {
                let entry = x.ok()?;
                let filename = entry.file_name();
                let expected_content = match filename.to_str()? {
                    "test.txt" => "bar",
                    "bar.txt" => "bar",
                    "recipes.json" => return None,
                    _ => panic!("Unknown file {filename:?}"),
                };

                let x = tokio::fs::read_to_string(entry.path()).await.ok()?;
                if x == expected_content {
                    Some(())
                } else {
                    None
                }
            })
            .count()
            .await
    );
}
