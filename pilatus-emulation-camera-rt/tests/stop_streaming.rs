#[cfg(feature = "integration")]
#[tracing_test::traced_test]
#[test]
fn stops_streaming_when_all_subscribers_are_gone() -> anyhow::Result<()> {
    use std::time::Duration;

    use futures::StreamExt;
    use image::{ImageBuffer, Rgb};
    use imbuf::DynamicImageChannel;
    use minfac::Registered;
    use pilatus::device::ActorSystem;
    use pilatus::{DeviceConfig, Recipe, Recipes};
    use pilatus_engineering::image::{DynamicImage, SubscribeDynamicImageMessage};
    use pilatus_rt::TempRuntime;
    use serde_json::json;
    use tokio::time::sleep;
    fn write_color_image(path: impl AsRef<std::path::Path>, color: [u8; 3]) -> anyhow::Result<()> {
        let img = ImageBuffer::from_pixel(2, 2, Rgb(color));
        img.save(path)?;
        Ok(())
    }

    fn pixel_color(image: &DynamicImage) -> [u8; 3] {
        let first = image.first();
        match (first, image.len().get(), first.pixel_elements().get()) {
            (DynamicImageChannel::U8(typed), 1, 3) => {
                let buf = typed.buffer_flat();
                [buf[0], buf[1], buf[2]]
            }
            other => panic!("unexpected image format: {other:?}"),
        }
    }

    let configured = TempRuntime::new()
        .config(serde_json::json!({
            "web": { "socket": "0.0.0.0:0" }
        }))
        .register(pilatus_emulation_camera_rt::register)
        .register(pilatus_engineering_rt::register)
        .register(pilatus_axum_rt::register)
        .configure()?;

    let mut recipe = Recipe::default();
    let camera_id = recipe.add_device(DeviceConfig::new_unchecked(
        "pilatus-emulation-camera",
        "Camera",
        json!({
            "file": {
                "interval": 5,
                "auto_restart": false
            }
        }),
    ));

    let recipes_dir = configured.path().join("recipes");
    std::fs::create_dir_all(&recipes_dir)?;
    std::fs::write(
        recipes_dir.join("recipes.json"),
        serde_json::to_string(&Recipes::new_with_recipe(recipe))?,
    )?;

    let device_dir = recipes_dir.join(camera_id.to_string());
    let collection_dir = device_dir.join("collection");
    std::fs::create_dir_all(&collection_dir)?;
    write_color_image(collection_dir.join("image0.png"), [255, 0, 0])?;

    configured.run_until(
        |Registered(actor_system): Registered<ActorSystem>| async move {
            use anyhow::Context;

            tokio::time::sleep(Duration::from_millis(10)).await;
            let mut stream = actor_system
                .ask(camera_id, SubscribeDynamicImageMessage::default())
                .await
                .context("subscribe stream")?;

            let first_frame = stream
                .next()
                .await
                .context("stream finished before first frame")?
                .context("stream returned error")?;
            assert_eq!(
                pixel_color(&first_frame.image),
                [255, 0, 0],
                "expected first frame to come from the first image"
            );
            drop(stream);

            sleep(Duration::from_millis(100)).await;
            tokio::fs::write(collection_dir.join("image1.png"), b"not a image").await?;
            sleep(Duration::from_millis(100)).await;
            assert!(!logs_contain("ERROR"),);
            Ok(())
        },
    )
}
