/// Macro to conditionally make structs and enums public based on the `unstable` feature flag.
///
/// When the `unstable` feature is enabled, the item is `pub` (replacing any existing visibility).
/// When the `unstable` feature is disabled, the original visibility modifier is preserved.
///
/// This allows unstable APIs to be used in tests and tightly-coupled crates while
/// keeping them private in stable releases.
///
/// # Examples
///
/// ```rust,ignore
/// pilatus::unstable_pub!(
///     struct Foo { }  // private when unstable disabled, pub when unstable enabled
/// );
///
/// pilatus::unstable_pub!(
///     pub(crate) struct Bar { }  // pub(crate) when unstable disabled, pub when unstable enabled
/// );
///
/// pilatus::unstable_pub!(
///     pub(super) struct Baz { }  // pub(super) when unstable disabled, pub when unstable enabled
/// );
/// ```
///
/// # Errors
///
/// This macro will fail to compile if the item is already `pub`:
///
/// ```rust,compile_fail
/// pilatus::unstable_pub!(
///     pub struct AlreadyPublic { }  // Error: item is already public
/// );
/// ```
#[macro_export]
macro_rules! unstable_pub {
    // Error case: already pub struct
    ($(#[$attr:meta])* pub struct $name:ident $($rest:tt)*) => {
        compile_error!("unstable_pub! macro cannot be used on items that are already `pub`. Remove the macro or change the visibility to something else (e.g., `pub(crate)`, `pub(super)`, or private).");
    };
    // Error case: already pub enum
    ($(#[$attr:meta])* pub enum $name:ident $($rest:tt)*) => {
        compile_error!("unstable_pub! macro cannot be used on items that are already `pub`. Remove the macro or change the visibility to something else (e.g., `pub(crate)`, `pub(super)`, or private).");
    };
    // Normal case: struct with visibility modifier (can be empty for private)
    ($(#[$attr:meta])* $vis:vis struct $name:ident $($rest:tt)*) => {
        $(#[$attr])*
        #[cfg(feature = "unstable")]
        pub struct $name $($rest)*
        $(#[$attr])*
        #[cfg(not(feature = "unstable"))]
        $vis struct $name $($rest)*
    };
    // Normal case: enum with visibility modifier (can be empty for private)
    ($(#[$attr:meta])* $vis:vis enum $name:ident $($rest:tt)*) => {
        $(#[$attr])*
        #[cfg(feature = "unstable")]
        pub enum $name $($rest)*
        $(#[$attr])*
        #[cfg(not(feature = "unstable"))]
        $vis enum $name $($rest)*
    };
}
