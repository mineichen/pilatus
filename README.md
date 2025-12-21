# Pilatus Framework
A modular, extendable application framework which is currently used for industrial and computervision projects. The core is not limited to engineering, but should suite other projects with autonomous configurable subsystems which interact with one another.

📚 **[Read the Documentation](https://mineichen.github.io/pilatus/)**

## Features
- Run asynchronous actors which communicate via message passing
- Switch between different sets of actor configurations without restarting the application using the built in recipe management
- Adding new extension don't require changes in the core.
- Easily expose core functionality through apis like http, websockets, opcua
- Multi-platform (Linux, OSX, Windows), multi-architecture (x64, arm)

## Modules
Currently, just a few modules are publicly available
- Axum: Collects web-routes from other modules an serves them on a single http port
- Engineering: Working with images and other engineering-related stuff like angles, matrices
- Soon: Leptos microfrontends
- Soon: Emulation of cameras

Some extensions which are implemented, but not (yet) publicly available:
- GigE/Ueye-Camera: Discovery, settings gige-params, image streams
- Image-Coordinates to 3D: Intrinsic calibration assistant, calibration plate management, origin determination
- Matching: Find shapes in images, which can be teached entirely in the web-ui (inkl. collision-detection if part has to be grippable by robot)
- Halcon: perform image analysis using hdpl-procedures, manage licenses, rust-bindings
- Various Feeder-Hardware: Anyfeeder, Aflex, Flexibowl, ...
- OpcUA: Let other modules define OpcUA endpoints

Reach out if you need such functionality