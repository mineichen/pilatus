# Structure

Core projects are devided into two cargo projects. The `{project-name}` and `{project-name}-rt`.

## rt-projects
Projects with the form `{name}-rt` mustn't be used as dependencies, except for executables and tests. All code which is required for interfacing lives in the `{name}` project.
The projects without suffix should only have minimal dependencies, so it can be used in other projects as e.g. `pilatus-leptos`.

If no `unstable` feature is set on the `*-rt` crate, it usually only provides a `register` method, which registers all of its dependencies to [minfac](https://docs.rs/minfac/latest/minfac/type.ServiceCollection.html)

## Unstable features
Many pilatus projects have a `unstable` feature. Code which is available behind these feature flags is not guaranteed to stay stable. They are used to:
 - Make code available for testing.
   - It's ok if tests of your crate break due to breaking changes in e.g. `pilatus-rt/unstable`. If `unstable` is only in `[dev-dependencies]` and not `[dependencies]`, your crate remains stable for others to be used.
 - Allow for tightly bound crates.
   - The GUI will be tightly coupled to a specific backend version internally. But their interface to dependents is just a stable leptos-component.
   - `{name}-rt` projects may depend on their `{name}` counterpart with active `unstable` feature.  Unstable can e.g. make {name}::DeviceParams available, which is a shining example of a unstable, ever evolving structures you shouldn't depend on in your project. But the UI and the `device` implementation in your `{name}-rt` project are allowed to rely on it.

## Why do we even have `-rt` projects?
The biggest reason for having `*-rt` projects is compilation time. RT projects often have significant dependencies. `pilatus-axum-rt` contains heavy dependencies like `tokio`, `async-zip`, `image`, `tower` etc. If all this code lived in `pilatus-axum`, depending crates could only start compiling, when `pilatus-axum-rt` and all of its dependencies are ready, leading to long dependency chains. For crates on which noone depends on, you could also add a `rt` feature flag to your `{name}` project instead. This allows:
 + fine grained visibility (e.g. Params-fields are only visible in a submodule)
 + avoiding Orphan-Rule problems

For device Extensions like `pilatus-emulation-camera` this might be the better option, because it will rarely be a dependency except in integration tests and executables.


## Ignore executables
In projects it often makes sense, not to add your executable to your workspace `default-members`. If you do, each change of your code triggers a rebuild of the executable, which is very slow to link on e.g. Windows.

## Maintain two Compilation modes
This all leads to pilatus having two dependency modes: default and integration

| | Default | Integration |
|---|---|---|
| **Usage**             | Building, Unittesting                        | Integration tests and executables                             |
| **Compilation speed** | Faster - excludes heavy runtime dependencies | Slower - includes all runtime dependencies                    |
| **Invocations**       | `cargo build`, `cargo test`                  | `cargo test --test integration`, `cargo run --bin executable` |

To maintain these characteristics, follow these guidelines
 - Don't add rt-deps to your [dev-dependencies], as optional dev-deps are not suported by cargo. Add them optional to `[features]` and enbable it with the `integration` feature
 - RT-Crates are allowed to add `unstable` feature to their non-rt counterpart
 - Add rules for your editor, which enables your "integration" feature for rust-analyzer (see this project for zed and vscode)
 - Check, that `unstable` is not used in your project, by `cargo check` ing each project separately. If you run it on the workspace, tests or the executable might enable `unstable` features

## Leptos UI
The frontend code is deliberately developed under the MIT License in a separate repository. You are free to use it as a template for your own frontend and modify it without the need to publish your changes.

The backend is licensed under the MPL-2.0. You may modify it for your own use, but any changes to the backend itself must be shared under the same license, promoting open collaboration and shared progress. The MPL-2.0 is a file-level copyleft license, which means you can combine the backend with proprietary code or extensions without affecting the license of your own proprietary components.
