# Release new dev-container version
Pilatus provides images for both aarch64 and x86-64 architectures.

## Build command
Before uploading a new image to docker-hub, try it on your local machine by overwriting the current tag and run the dev-container.
```
docker buildx build --platform linux/arm64,linux/amd64 -t mineichen/pilatus-build:0.0.1.81 --push
```


## Setup docker environment for buildx
```
docker buildx create --use
docker run --privileged --rm tonistiigi/binfmt --install all
docker login -u mineichen

```

# Troubleshooting
If a cc command fails to build a crate on arm, it's most likely a virtualization-error and not a problem with cargo/rust. On OSX, binfmt solved the issue.