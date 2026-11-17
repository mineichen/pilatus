#v0.2
- Add `RelativeResourcePath(Buf)`. Deprecate `RelativeFilePath` (misleading name, it contained a PathBuf)
- `RelativeDirPathError` and `RelativeFilePathError` contain the constructor input instead of `String`
- Remove `sealedstruct::Seal` and `PercentageRaw` from `Percentage`
- Emulation-Camera reencodes every image to PNG and strips all metadata
- Emulation-Camera stops immediately when the device is stopped, instead of after the next frame interval
- Fix default logo colors (light: black contours, dark: white contours). Rename logo theme `bright` to `light`
- Update `imbuf` to 0.4: Images have a lifetime (`Image<T, N>` = `ImageView<'static, T, N>`, `DynamicImage` = `DynamicImageView<'static>`). Conversions from the `image` crate moved to `imbuf-image`
- `SubscribeDynamicImageMessage` is a struct instead of an alias. Add `SubscribeImageWithMetaMessage<T, CHANNELS>`
- `ask` of `ActorSystem` and all senders returns `impl Future + Send + 'static`, so it's `Send` for messages with lifetimes. `ActorMessageSender::ask` and `UntypedActorMessageSender::ask` send immediately instead of on the first poll
- `ExtractWithFormatError::Unsupported` contains the image and `imbuf::IncompatibleImageError`. `Rgb8Image::try_from(DynamicImage)` fails with `imbuf::IncompatibleImage<DynamicImage>`
- Require `stream-broadcast` 0.3.1 (`StreamBroadcastLossy`)
- Add `FileService::remove_file_if_exists` (returns whether the file existed)
- Add `SystemShutdown::run_or_shutdown`: Runs a future unless the system shuts down first (`ShutdownError`)

### Recipes
- Rename `RecipeServiceTrait` to `RecipeServiceFacade` (deprecated subtrait kept). Its methods take options explicitly and are named like the former `*_with` methods (breaking for implementors)
- `RecipeService` is a cloneable struct (`RecipeService::new(Arc<dyn RecipeServiceFacade + Send + Sync>)`) instead of an alias (breaking). It has the `*_with` methods (accepting `impl Into<...Options>`) and variants with default options
- Rename `RecipeServiceFassade(Builder)` of pilatus-rt to `RecipeServiceImpl(Builder)` (deprecated aliases kept)
- Rename `RecipeService::activate_recipe(_with)` to `start_recipe(_with)` (deprecated aliases kept)
- Add `PendingChangesStrategy` (`Deny` (default), `Discard`, `Commit`) and `StartRecipeOptions { strategy, transaction }`, which `RecipeServiceFacade::start_recipe` takes instead of `TransactionOptions` (breaking)
- `start_recipe` stops all devices, resolves uncommitted changes of the active recipe with the strategy and restarts the devices, even if the recipe is active already or the change was denied. An unknown recipe fails without touching the devices
- HTTP: `PUT /api/recipe/start/{id}?pending_changes=deny|discard|commit` answers `409 Conflict` if changes were denied. `GET` answers `301` and does nothing (breaking)
- The id of the active recipe can't be changed anymore (breaking). Its tags are no uncommitted changes and are kept when committing or discarding
- Deprecate `RecipeService::restore_active(_with)`, which always failed. Remove it from `RecipeServiceFacade` and `PUT /api/recipe/restore` (breaking). Use `PendingChangesStrategy::Discard`
- Deprecate `RecipeRunner(Trait)`. Use `RecipeService::start_recipe` (`select_recipe` behaves like `Deny`)
- Add `Recipes::restore_active` and `Recipes::active_backup`
- `UncommittedChangesError` can be detected in `TransactionError::Other` (`anyhow::Error::is`). `TransactionOptions` implements `Debug`
