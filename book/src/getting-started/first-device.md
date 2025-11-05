# Your First Device

## Add new dependencies to your library project
```bash
cargo add pilatus --git https://github.com/mineichen/pilatus.git --features tokio
cargo add serde --features derive
cargo add anyhow
cargo add tracing
```

## Create camera submodule

We are now finally getting to implement the custom camera. To do so, we'll add a new module `camera.rs` in our extension library.


```bash
use minfac::ServiceCollection;

pub(super) fn register_services(c: &mut ServiceCollection) {
    // TODO: Implement the camera service
}
```

Add the mod to your `lib.rs` and call the `regsiter_services` from within the `register` function:
```
pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    // ... Existing code
    camera::register_services(collection);
}

```
Now run `cargo run` in your app to make sure, everything still works fine.

## Register the device
It is now time to create and register a device. 

```rust
use minfac::{Registered, ServiceCollection};
use pilatus::{
    UpdateParamsMessageError,
    device::{ActorSystem, DeviceContext, DeviceValidationContext, ServiceBuilderExtensions},
};
use serde::{Deserialize, Serialize};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<ActorSystem>>()
        .register_device("my_camera", validator, device);
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Params {
    url: String,
}

async fn validator(ctx: DeviceValidationContext<'_>) -> Result<Params, UpdateParamsMessageError> {
    ctx.params_as::<Params>()
}

async fn device(
    ctx: DeviceContext,
    params: Params,
    actor_system: ActorSystem,
) -> anyhow::Result<()> {
    //actor_system.register(ctx.id).execute(params).await;
    tracing::info!("Start Camera device with params: {:?}", params);
    actor_system.register(ctx.id).execute(params).await;
    tracing::info!("Camera is shutting down");
    Ok(())
}

```

While this compiles already, it's never doing anything yet. Devices are only run, if they are part of a so called `recipe`. By default, there is only one recipe without any devices at all. In practice, you can add devices to existing recipes via api calls or you can overwrite the default recipe, so it contains the devices your specific app needs. A Recipe can also contain multiple Instances of the same Device (e.g. 2 Cameras). To dig deeper, have a look at [Recipes](../core-concepts/recipes.md).

## Run the device
By default, pilatus writes all of it's files into the `data` directory. If this folder doesn't exist, it will be created when starting the executable for the first time. During development, its very convenient to have it in the app folder, but in practice, the app is often installed at a different location, where the process itself doesn't have write access (e.g. Programs ond windows). The location might even be installation specific (User/System installation). The data folder should usually not be checked into git and therefore be part of your .gitignore file.

To add a device, we are going to edit the `my-pilatus-app/data/recipes/recipes.json` file. Your file should look roughly the same, but `devices` was still empty.

``` json
{
  "active_id": "default",
  "active_backup": {
    "created": "2025-11-05T12:27:15.645499788Z",
    "tags": [],
    "devices": {}
  },
  "all": {
    "default": {
      "created": "2025-11-05T12:27:15.645499788Z",
      "tags": [],
      "devices": {
        "11111111-1111-1111-1111-111111111111": {
          "device_type": "my_camera",
          "device_name": "BirdCamera",
          "params": {
            "url": "https://media.istockphoto.com/id/539648544/photo/eastern-bluebird-sialia-sialis-male-bird-in-flight.webp?b=1&s=612x612&w=0&k=20&c=BcPh4xbjrDVTyiErKB8RZFQ3quuME-4vDSnZRu09xCQ="
          }
        },
        "22222222-2222-2222-2222-222222222222": {
          "device_type": "my_camera",
          "device_name": "FoxCamera",
          "params": {
            "url": "https://images.unsplash.com/photo-1474511320723-9a56873867b5?ixlib=rb-4.1.0&ixid=M3wxMjA3fDB8MHxwaG90by1wYWdlfHx8fGVufDB8fHx8fA%3D%3D&auto=format&fit=crop&q=80&w=1172"
          }
        }
      }
    }
  },
  "variables": {}
}
```
If you run the app again, you should see two tracing infos in the logs indicating, that both are running. Nice, so we are getting close.

## Handling messages
The device currently only waits on `execute` to shut itself down. So we cannot interact with the device yet.

To do so, we are going to create a ActorMessage. ActorMessages are the protocol, which allows interfaces (e.g. Web-route) or other Actors to communicate with one other. As a simple example, we're going to add a GetImageUri message, which only makes sense for cameras, which load images from e.g. file or http.

```rust
struct GetImageUrlMessage;
impl ActorMessage for GetImageUrlMessage {
    type Output = pilatus_axum::http::Uri;
    type Error = anyhow::Error;
}

impl Params {
    async fn handle_image_url(
        &mut self,
        _msg: GetImageUrlMessage,
    ) -> ActorResult<GetImageUrlMessage> {
        Uri::from_str(&self.url).map_err(ActorError::custom)
    }
}

```

Next, we need to tell our device to handle such messages, if someone asks them.
This is just one more registration inside the device function. Just replace


```rust
actor_system.register(ctx.id).execute(params).await;

```
with

```rust
 actor_system
    .register(ctx.id)
    .add_handler(Params::handle_image_url)
    .execute(params)
    .await;

```

The actor system makes sure, only one message handler runs at a time and grants mutable access to the device state while the message is beign processed. Notice, that you don't have to deal with mutexes, as this is handled in the ActorSystem.

At this point, we could call this device from another device, but we don't yet have another device. So we are creating a http route to do so.


## Add a Http-Route
We are going to expose another http endpoint, which is calling the ActorSystem with the GetImageUrl Message and returns that response via HTTP. 
``` rust

pub(super) fn register_services(c: &mut ServiceCollection) {
    // ... Device registration
    c.register_web("my_camera", |r| r.http("/image_url", |m| m.get(get_image_url_web)));
}

async fn get_image_url_web(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    Query(id): Query<DynamicIdentifier>,
) -> impl IntoResponse {
    DeviceResponse::from(
        actor_system
            .ask(id, GetImageUrlMessage)
            .await
            .map(|url| url.to_string()),
    )
}

```

## Test the message handler via HTTP
``` bash
curl http://localhost:8080/api/my_camera/image_url?device_id=11111111-1111-1111-1111-111111111111
curl http://localhost:8080/api/my_camera/image_url?device_id=22222222-2222-2222-2222-222222222222
```

You shoud see the image urls as output.

## Summary
Here is the full code of the camera example
```rust
use std::str::FromStr;

use minfac::{Registered, ServiceCollection};
use pilatus::{
    UpdateParamsMessageError,
    device::{
        ActorError, ActorMessage, ActorResult, ActorSystem, DeviceContext, DeviceValidationContext,
        DynamicIdentifier, ServiceBuilderExtensions,
    },
};
use pilatus_axum::{
    DeviceResponse, IntoResponse, ServiceCollectionExtensions,
    extract::{InjectRegistered, Query},
    http::Uri,
};
use serde::{Deserialize, Serialize};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<ActorSystem>>()
        .register_device("my_camera", validator, device);
    c.register_web("my_camera", |r| {
        r.http("/image_url", |m| m.get(get_image_url_web))
    });
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Params {
    url: String,
}

async fn validator(ctx: DeviceValidationContext<'_>) -> Result<Params, UpdateParamsMessageError> {
    ctx.params_as::<Params>()
}

async fn device(
    ctx: DeviceContext,
    params: Params,
    actor_system: ActorSystem,
) -> anyhow::Result<()> {
    tracing::info!("Start Camera device with params: {:?}", params);
    actor_system
        .register(ctx.id)
        .add_handler(Params::handle_image_url)
        .execute(params)
        .await;
    tracing::info!("Camera is shutting down");
    Ok(())
}

struct GetImageUrlMessage;
impl ActorMessage for GetImageUrlMessage {
    type Output = pilatus_axum::http::Uri;
    type Error = anyhow::Error;
}

impl Params {
    async fn handle_image_url(
        &mut self,
        _msg: GetImageUrlMessage,
    ) -> ActorResult<GetImageUrlMessage> {
        Uri::from_str(&self.url).map_err(ActorError::custom)
    }
}

async fn get_image_url_web(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    Query(id): Query<DynamicIdentifier>,
) -> impl IntoResponse {
    DeviceResponse::from(
        actor_system
            .ask(id, GetImageUrlMessage)
            .await
            .map(|url| url.to_string()),
    )
}
```

## Next steps

Congratullations! You implemented your first device and added a external interface to it. I'd recommend you to go to the Core concepts section to find out more about [devices](../core-concepts/devices.md) and [recipes](../core-concepts/recipes.md).