# Your First Application

Let's create a simple Pilatus application to get familiar with the framework.

## Creating a New Project

First, create a new Rust project:

```bash
cargo new my-pilatus-app
cd my-pilatus-app
cargo add pilatus-rt --git https://github.com/mineichen/pilatus.git
cargo add pilatus-axum-rt --git https://github.com/mineichen/pilatus.git
```

## A Simple Example

Here's a minimal example to get you started, which includes the axum webserver:

```rust
fn main() {
    pilatus_rt::Runtime::default()
        .register(pilatus_axum_rt::register)
        .run();
}

```

## Run the app

```bash
cargo run
```

> **Troubleshooting: Port 80 requires elevated privileges**
>
>> ERROR: Hosted service 'Main Webserver' failed: Cannot open TCP-Connection for webserver. Is pilatus running already?
>
> By default, Pilatus uses port 80 for the web interface. On some systems, this requires running with elevated privileges (sudo/root). 
> To use a different port, create a `data/config.json` with the following content
>
> ```json
> {
>   "web": {
>     "socket": "0.0.0.0:8080"
>   }
> }
> ```

## Test the app

```bash
curl http://localhost:80/api/time
```

This should show you the time on the system where the server is running. Congratulation, you just run your first pilatus app. 

## Next Steps

Now that you have a basic setup, we can add your first extension


