## Your First Extension

## Create a new project
Pilatus encourages you, to split your code into multiple separate creates. So we are gonna do that here:

``` bash
cargo init --lib my-pilatus-extension-rt
cargo add pilatus-axum --git https://github.com/mineichen/pilatus.git
cargo add minfac

```

## Create a extension
Pilatus heavily relies on the [minfac](http://crates.io/crates/minfac) inversion of control library. It allows making concepts to other code available. We are gonna add a new http route to the my-pilatus-app. To do so, we change the `lib.rs` to:


``` rust
use pilatus_axum::{IntoResponse, ServiceCollectionExtensions};

pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    collection.register_web("my-first-extension", |r| {
        r.http("/greet", |m| m.get(get_response))
    });
}

async fn get_response() -> impl IntoResponse {
    "Hello, World!"
}
```



## Register the extension in our app
Go to `my-pilatus-app` project and add this library to the dependencies
``` bash
cd ../my-pilatus-app
cargo add my-pilatus-extension-rt --path ../my-pilatus-extension-rt
```

To use your new extension, you just add a `my_pilatus_extension_rt` to your main.rs. It should now look something like this:
``` rust
fn main() {
    pilatus_rt::Runtime::default()
        .register(pilatus_axum_rt::register)
        .register(my_pilatus_extension_rt::register)
        .run();
}
```

The ordering of register-calls is not relevant here. Ordering is only relevant, if your extension would overwrite core services, which is a very advanced topic.

## Test your app
```bash
curl http://localhost:80/api/my-first-extension/greet
```

This should output `hello world` in your terminal.


## How this works
Pilatus extensions just register data into the `minfac::ServiceCollection` (e.g. a struct, TraitObject or even a primitive). In this case`register_web` builds a specific struture with the routing information and registers it to minfac. When the webserver is booting up, it gathers all registered instances of this type and provides them with a axum webserver.


## Next Steps

Congratullations, you just wrote your first pilatus extension. If you want a deeper understanding on how the extension system works, take a look at [minfac](http://crates.io/crates/minfac). If you are comfortable to accept, that this feels a little magic right now, I encourage you to just continue. In the next chapter, where we build our first device, you should feel much more comfortable.

