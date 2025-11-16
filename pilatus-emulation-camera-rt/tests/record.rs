#[cfg(feature = "integration")]
#[test]
fn record_integration() -> anyhow::Result<()> {
    use futures::StreamExt;
    use image::Rgb;
    use pilatus::{DeviceConfig, Recipe, Recipes, RelativeDirectoryPath};
    use pilatus_rt::{Runtime, TokioFileService};
    use serde_json::json;

    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("config.json"),
        br#"{
            "tracing": {
                "default_level": "debug",
                "filters": {
                  "tokio": "info",
                  "pilatus::image": "debug"
                }
            }
        }"#,
    )?;
    let mut recipe = Recipe::default();
    let player_id = recipe.add_device(DeviceConfig::new_unchecked(
        "engineering-emulation-camera",
        "Player",
        json!({}),
    ));
    let recorder_id = recipe.add_device(DeviceConfig::new_unchecked(
        "engineering-emulation-camera",
        "Recorder",
        json!({
            "permanent_recording": {
                "collection_name": "foo",
                "source_id": player_id,
            }
        }),
    ));
    let recipes_path = dir.path().join("recipes");
    let player_collection_path = recipes_path.join(player_id.to_string()).join("bar");
    std::fs::create_dir_all(&player_collection_path)?;

    std::fs::write(
        recipes_path.join("recipes.json"),
        serde_json::to_string(&Recipes::new_with_recipe(recipe))?,
    )?;

    for (i, color) in [[0u8, 0, 255], [0, 255, 0], [255, 0, 0]]
        .into_iter()
        .enumerate()
    {
        let image = image::ImageBuffer::from_pixel(2, 2, Rgb(color));
        image.save(player_collection_path.join(format!("imgage{i}.png")))?;
    }
    Runtime::with_root(dir.path())
        .register(pilatus_emulation_camera_rt::register)
        .configure()
        .run_until_finished(async {
            let file_service = TokioFileService::builder(recipes_path).build(recorder_id);
            for _ in 0..3 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let all = file_service
                    .stream_files_recursive(RelativeDirectoryPath::new("foo").unwrap())
                    .filter_map(|x| async { x.ok() })
                    .collect::<Vec<_>>()
                    .await;

                if all.len() >= 2 {
                    println!(
                        "Files: {:?}",
                        all.iter().map(|f| f.file_name()).collect::<Vec<_>>()
                    );
                    let data = file_service.get_file(all.get(0).unwrap()).await.unwrap();
                    let image = image::load_from_memory(&data).unwrap();
                    assert!(
                        matches!(image, image::DynamicImage::ImageRgb8(_)),
                        "Not rgb image: {image:?}"
                    );
                    return;
                }
            }
            panic!("Never got more than 1 file");
        });
    Ok(())
}
