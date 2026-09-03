#![cfg(feature = "integration")]
use testresult::TestResult;

#[test]
fn file_api_endpoints() -> TestResult {
    use image::{ImageBuffer, Rgb};
    use minfac::Registered;
    use pilatus::{DeviceConfig, Recipe};
    use pilatus_rt::TempRuntime;
    use serde_json::json;

    fn create_png(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
        let image = ImageBuffer::from_pixel(width, height, Rgb(color));
        let mut data = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut data),
                image::ImageFormat::Png,
            )
            .unwrap();
        data
    }

    let mut recipe = Recipe::default();
    let device_id = recipe.add_device(DeviceConfig::new_unchecked(
        "pilatus-emulation-camera",
        "Camera",
        json!({
            "file": {
                "interval": 50
            }
        }),
    ));

    let configured = TempRuntime::new()
        .config(serde_json::json!({ "web": { "socket": "0.0.0.0:0" } }))
        .add_recipe(recipe)
        .register(pilatus_emulation_camera_rt::register)
        .register(pilatus_engineering_rt::register)
        .register(pilatus_axum_rt::register)
        .configure()?;

    let recipes_dir = configured.path().join("recipes");
    let device_dir = recipes_dir.join(device_id.to_string());
    let collection_dir = device_dir.join("test_collection");
    std::fs::create_dir_all(&collection_dir)?;

    std::fs::write(collection_dir.join("b.png"), create_png(2, 2, [255, 0, 0]))?;
    std::fs::write(collection_dir.join("a.png"), create_png(3, 3, [0, 255, 0]))?;
    std::fs::write(device_dir.join("root.png"), create_png(1, 1, [0, 0, 255]))?;

    configured.run_until(
        |Registered(web_stats): Registered<pilatus_axum::Stats>| async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let port = web_stats.socket_addr().await.port();
            let base_url = format!("http://127.0.0.1:{port}/api/recipe/file/{device_id}");
            let client = reqwest::Client::new();
            let collection_url =
                format!("http://127.0.0.1:{port}/api/recipe/file/list/{device_id}/test_collection");

            async fn list_images(client: &reqwest::Client, url: &str) -> TestResult<Vec<String>> {
                let response = client.get(url).send().await?;
                assert!(
                    response.status().is_success(),
                    "Listing files failed with status: {}",
                    response.status()
                );
                let mut files: Vec<String> = serde_json::from_str(&response.text().await?)?;
                files.sort();
                Ok(files)
            }

            // List all files of the collection (paths are relative to the device root)
            let files = list_images(&client, &collection_url).await?;
            assert_eq!(
                files,
                vec![
                    "test_collection/a.png".to_string(),
                    "test_collection/b.png".to_string()
                ]
            );

            // Get the raw bytes of an existing image
            let image_url = format!("{base_url}/test_collection/b.png");
            let response = client
                .get(&image_url)
                .send()
                .await
                .expect("Failed to send get file request");
            assert!(
                response.status().is_success(),
                "Getting file failed with status: {}",
                response.status()
            );
            assert_eq!(
                response.bytes().await.expect("Expected file bytes").len(),
                create_png(2, 2, [255, 0, 0]).len()
            );

            // Add a valid image through the standard endpoint
            let response = client
                .put(format!("{base_url}/test_collection/uploaded.png"))
                .body(create_png(4, 4, [0, 255, 0]))
                .send()
                .await
                .expect("Failed to send add file request");
            assert!(
                response.status().is_success(),
                "Adding file failed with status: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            );
            assert!(collection_dir.join("uploaded.png").exists());

            // Overwriting an image replaces its content all-or-nothing
            let response = client
                .put(format!("{base_url}/test_collection/uploaded.png"))
                .body(create_png(5, 5, [0, 0, 255]))
                .send()
                .await
                .expect("Failed to send add file request");
            assert!(
                response.status().is_success(),
                "Overwriting file failed with status: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            );
            assert_eq!(
                std::fs::read(collection_dir.join("uploaded.png"))?.len(),
                create_png(5, 5, [0, 0, 255]).len()
            );

            // Adding a file with invalid image data is rejected by the validator
            let response = client
                .put(format!("{base_url}/test_collection/invalid.png"))
                .body(b"this is not an image".to_vec())
                .send()
                .await
                .expect("Failed to send add file request");
            assert!(
                !response.status().is_success(),
                "Adding invalid image should fail"
            );
            assert!(!collection_dir.join("invalid.png").exists());

            // Adding/Deleting files outside of collections is not allowed
            let response = client
                .put(format!("{base_url}/root.png"))
                .body(create_png(1, 1, [0, 0, 255]))
                .send()
                .await
                .expect("Failed to send add file request");
            assert!(
                !response.status().is_success(),
                "Adding a file at the device root should be denied"
            );
            let response = client
                .delete(format!("{base_url}/root.png"))
                .send()
                .await
                .expect("Failed to send delete file request");
            assert!(
                !response.status().is_success(),
                "Deleting a file at the device root should be denied"
            );
            assert!(device_dir.join("root.png").exists());

            // Delete a single image through the standard endpoint
            let response = client
                .delete(format!("{base_url}/test_collection/a.png"))
                .send()
                .await
                .expect("Failed to send delete file request");
            assert!(
                response.status().is_success(),
                "Deleting file failed with status: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            );
            assert!(!collection_dir.join("a.png").exists());
            assert!(collection_dir.join("b.png").exists());

            // Listing reflects the changes
            let files = list_images(&client, &collection_url).await?;
            assert_eq!(
                files,
                vec![
                    "test_collection/b.png".to_string(),
                    "test_collection/uploaded.png".to_string()
                ]
            );

            println!("Successfully tested the standard file api!");
            Ok(())
        },
    )
}
