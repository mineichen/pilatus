# Evaluation

## Requirements
 1. Async on Tokio, because the webserver is just one component in a big distributed system
 2. Support WebSockets
 3. Wrappability. Code generation by macros should be minimal, as WebHooks will be registered with minfac
 4. longevity
 5. (no strict requirement) OpenApi


 ## Contenders
lib     |1|2|3|4|5|
poem     x x x   x 
axum     x x x x   
warp     x x x 
rocket   x x   x x
rweb     x x x   x   
actix      x   x

Axum was choosen, because it's maintained by tokio itself and is therefore more likely to remain in the long run. OpenApi is likely to be added to axum in the future (https://github.com/tokio-rs/axum/tree/axum-openapi-crate)

# Simple interface is more important than conveniences
As web-hooks should be added from other crates, the interface should remain free of any framework specifics. The minimum required information like path, httpverb and handler are adapted to the framework in this crate.