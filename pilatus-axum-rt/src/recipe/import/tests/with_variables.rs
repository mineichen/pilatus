use std::sync::Arc;

use futures::io::Cursor;
use pilatus::{
    DeviceConfig, ImportRecipeError, ImportRecipesOptions, Name, RecipeExporterTrait,
    RecipeServiceTrait,
};
use pilatus::{ParameterUpdate, UntypedDeviceParamsWithVariables, VariableConflict};
use pilatus_rt::RecipeServiceFassade;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::recipe::import::ZipReaderWrapper;

#[derive(Deserialize, Serialize)]
struct State {
    number: i32,
    text: String,
}

#[tokio::test]
async fn with_variables() {
    let (_dir, rsb) = RecipeServiceFassade::create_temp_builder();
    let rs = Arc::new(rsb.build());
    let active_recipe_id = rs.get_active_id().await;

    let (export_recipe_id, _) = rs.duplicate_recipe(active_recipe_id).await.unwrap();

    let device_id = rs
        .add_device_to_recipe(
            export_recipe_id.clone(),
            DeviceConfig::mock(State {
                number: 1,
                text: "initial".into(),
            }),
        )
        .await
        .unwrap();

    let var_json: UntypedDeviceParamsWithVariables = serde_json::from_value(
        json!( { "number": {"__var": "number1"}, "text": {"__var": "text1"}}),
    )
    .unwrap();
    rs.update_device_params(
        export_recipe_id.clone(),
        device_id,
        ParameterUpdate {
            parameters: var_json.clone(),
            variables: [
                (Name::new("number1").unwrap(), 1.into()),
                (Name::new("text1").unwrap(), "initial_text".into()),
            ]
            .into_iter()
            .collect(),
        },
    )
    .await
    .unwrap();

    let rs_clone = rs.clone();
    let export_recipe_id_clone = export_recipe_id.clone();
    let data = super::writer_into_vec_unchecked(move |w| {
        let rs = rs_clone;
        async move { rs.export(export_recipe_id, w).await }
    })
    .await;
    //tokio::io::AsyncWriteExt::write_all(
    //    &mut tokio::fs::File::create("recipe.zip").await.unwrap(),
    //    &data,
    //)
    //.await
    //.unwrap();

    rs.update_device_params(
        export_recipe_id_clone.clone(),
        device_id,
        ParameterUpdate {
            parameters: var_json.clone(),
            variables: [(Name::new("text1").unwrap(), "other_text".into())]
                .into_iter()
                .collect(),
        },
    )
    .await
    .unwrap();

    rs.delete_recipe(export_recipe_id_clone).await.unwrap();

    let r = rs
        .create_importer()
        .import(
            &mut ZipReaderWrapper::new(Cursor::new(data.clone())),
            ImportRecipesOptions::default(),
        )
        .await;
    let _importer = if let Err(ImportRecipeError::Conflicts(_, x, i)) = r {
        assert_eq!(
            vec![VariableConflict {
                name: Name::new("text1").unwrap(),
                existing: "initial_text".into(),
                imported: "other_text".into()
            }],
            x
        );

        i
    } else {
        panic!("Expected ConflictingRecipes-error, got {r:?}");
    };

    assert_eq!(1, rs.state().await.recipes().iter_without_backup().count());
    //importer.apply(&rs, IntoMergeStrategy::Replace)
}
