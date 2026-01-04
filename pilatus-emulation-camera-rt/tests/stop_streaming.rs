#[cfg(feature = "integration")]
#[tracing_test::traced_test]
#[test]
fn stops_streaming_when_all_subscribers_are_gone() -> anyhow::Result<()> {
    use std::time::Duration;

    use futures::StreamExt;
    use image::{ImageBuffer, Rgb};
    use imbuf::DynamicImageChannel;
    use pilatus::device::ActorSystem;
    use pilatus::{DeviceConfig, Recipe, Recipes};
    use pilatus_engineering::image::{DynamicImage, SubscribeDynamicImageMessage};
    use pilatus_rt::Runtime;
    use serde_json::json;
    use tokio::time::sleep;
    fn write_color_image(path: impl AsRef<std::path::Path>, color: [u8; 3]) -> anyhow::Result<()> {
        let img = ImageBuffer::from_pixel(2, 2, Rgb(color));
        img.save(path)?;
        Ok(())
    }

    fn pixel_color(image: &DynamicImage) -> [u8; 3] {
        let first = image.first();
        match (first, image.len(), first.pixel_elements().get()) {
            (DynamicImageChannel::U8(typed), 1, 3) => {
                let buf = typed.buffer_flat();
                [buf[0], buf[1], buf[2]]
            }
            other => panic!("unexpected image format: {other:?}"),
        }
    }

    let tmp = tempfile::tempdir()?;
    std::fs::write(tmp.path().join("config.json"), "{}")?;

    let mut recipe = Recipe::default();
    let device_id = recipe.add_device(DeviceConfig::new_unchecked(
        "engineering-emulation-camera",
        "Camera",
        json!({
            "file": {
                "interval": 5,
                "auto_restart": false
            }
        }),
    ));

    let recipes_dir = tmp.path().join("recipes");
    std::fs::create_dir_all(&recipes_dir)?;
    std::fs::write(
        recipes_dir.join("recipes.json"),
        serde_json::to_string(&Recipes::new_with_recipe(recipe))?,
    )?;

    let device_dir = recipes_dir.join(device_id.to_string());
    let collection_dir = device_dir.join("collection");
    std::fs::create_dir_all(&collection_dir)?;
    write_color_image(collection_dir.join("image0.png"), [255, 0, 0])?;

    let runtime = Runtime::with_root(tmp.path())
        .register(pilatus_emulation_camera_rt::register)
        .register(pilatus_engineering_rt::register)
        .register(pilatus_axum_rt::register)
        .configure();

    let actor_system: ActorSystem = runtime.provider.get().unwrap();
    let camera_id = device_id;

    runtime.run_until_finished(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let mut stream = actor_system
            .ask(camera_id, SubscribeDynamicImageMessage::default())
            .await
            .expect("subscribe stream");

        let first_frame = stream
            .next()
            .await
            .expect("stream finished before first frame")
            .expect("stream returned error");
        assert_eq!(
            pixel_color(&first_frame.image),
            [255, 0, 0],
            "expected first frame to come from the first image"
        );
        drop(stream);

        sleep(Duration::from_millis(100)).await;
        tokio::fs::write(collection_dir.join("image1.png"), b"not a image")
            .await
            .unwrap();
        sleep(Duration::from_millis(100)).await;
        assert!(!logs_contain("ERROR"),);
    });

    Ok(())
}
