#[cfg(feature = "integration")]
#[test]
fn upload_image_to_collection() -> anyhow::Result<()> {
    use image::{GenericImageView, ImageBuffer, Rgb};
    use pilatus::{DeviceConfig, Recipe, Recipes};
    use pilatus_rt::Runtime;
    use serde_json::json;

    let tmp = tempfile::tempdir()?;
    std::fs::write(
        tmp.path().join("config.json"),
        br#"{ "web": { "socket": "0.0.0.0:0" } }"#,
    )?;

    let mut recipe = Recipe::default();
    let device_id = recipe.add_device(DeviceConfig::new_unchecked(
        "engineering-emulation-camera",
        "Camera",
        json!({
            "file": {
                "interval": 50
            }
        }),
    ));

    let recipes_dir = tmp.path().join("recipes");
    std::fs::create_dir_all(&recipes_dir)?;
    std::fs::write(
        recipes_dir.join("recipes.json"),
        serde_json::to_string(&Recipes::new_with_recipe(recipe))?,
    )?;

    // Create an initial collection with one image
    let device_dir = recipes_dir.join(device_id.to_string());
    let collection_dir = device_dir.join("test_collection");
    std::fs::create_dir_all(&collection_dir)?;

    let initial_image = ImageBuffer::from_pixel(2, 2, Rgb([255u8, 0, 0]));
    initial_image.save(collection_dir.join("initial.png"))?;

    let runtime = Runtime::with_root(tmp.path())
        .register(pilatus_emulation_camera_rt::register)
        .register(pilatus_axum_rt::register)
        .configure();

    let web_stats: pilatus_axum::Stats = runtime.provider.get().unwrap();

    runtime.run_until_finished(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let port = web_stats.socket_addr().await.port();
        let base_url = format!("http://127.0.0.1:{port}/api");

        // Create a new image to upload
        let upload_image = ImageBuffer::from_pixel(3, 3, Rgb([0u8, 255, 0]));
        let mut upload_data = Vec::new();
        upload_image
            .write_to(
                &mut std::io::Cursor::new(&mut upload_data),
                image::ImageFormat::Png,
            )
            .unwrap();

        // Upload the image via the HTTP endpoint
        let client = reqwest::Client::new();
        let upload_url = format!(
            "{}/engineering/emulation-camera/collection/test_collection/uploaded?device_id={}",
            base_url, device_id
        );

        let response = client
            .post(&upload_url)
            .body(upload_data)
            .send()
            .await
            .expect("Failed to send upload request");

        assert!(
            response.status().is_success(),
            "Upload failed with status: {} - {}",
            response.status(),
            response.text().await.unwrap_or_default()
        );

        // Verify the file was created (the handler adds .png extension)
        let uploaded_path = collection_dir.join("uploaded.png");
        assert!(
            uploaded_path.exists(),
            "Uploaded image file should exist at {:?}",
            uploaded_path
        );

        // Verify we can load the uploaded image
        let loaded = image::open(&uploaded_path).expect("Should be able to load uploaded image");
        assert_eq!(loaded.dimensions(), (3, 3), "Image dimensions should match");

        println!("Successfully uploaded and verified image!");
    });

    Ok(())
}
