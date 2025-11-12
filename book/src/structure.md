# Structure

Projects are usually devided into two cargo projects. The `{project-name}` and `{project-name}-rt`.

## rt-projects
Projects with the form `{name}-rt` mustn't be used as dependencies, except for executables and tests. All code which is required for interfacing lives in the `{name}` project.
The projects without suffix should only have minimal dependencies, so it can be used in other projects as e.g. `pilatus-leptos`.

## Unstable features
Many pilatus projects have a `unstable` feature. Code which is available behind these feature flags is not guaranteed to stay stable. They are used to:
 - Make code available for testing.
   - It's ok if tests of your crate break due to breaking changes in e.g. `pilatus-rt/unstable`. If `unstable` is only in `[dev-dependencies]` and not `[dependencies]`, your crate remains stable for others to be used.
 - Allow for tightly bound crates.
   - The GUI will be tightly coupled to a specific backend version internally. But their interface to dependents is just a stable leptos-component.
   - `{name}-rt` projects may depend on their `{name}` counterpart with active `unstable` feature.  Unstable can e.g. make {name}::DeviceParams available, which is a shining example of a unstable, ever evolving structures you shouldn't depend on in your project. But the UI and the `device` implementation in your `{name}-rt` project are allowed to rely on it.
