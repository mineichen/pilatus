use minfac::ServiceCollection;
use pilatus::{
    PendingChangesStrategy, RecipeId, RecipeService, TransactionError, UncommittedChangesError,
};
use pilatus_axum::{
    extract::{InjectRegistered, Path, Query},
    http::StatusCode,
    ServiceCollectionExtensions,
};

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("recipe", |r| r
        .http("/start/{id}", |m| m.get(moved_to_put).put(start_recipe))
    );
}

/// Starting a recipe is not safe, so it must not be reachable by a simple GET (link, prefetcher, `<img>`)
async fn moved_to_put() -> (StatusCode, &'static str) {
    (
        StatusCode::MOVED_PERMANENTLY,
        "Moved permanently: Use PUT instead of GET to start a recipe",
    )
}

#[derive(serde::Deserialize)]
struct StartRecipeQuery {
    /// `deny` (default), `discard` or `commit`
    #[serde(default)]
    pending_changes: PendingChangesStrategy,
}

/// Answers `409 Conflict`, if uncommitted changes were denied: The client can ask the user,
/// whether to send the request again with `discard` or `commit`
async fn start_recipe(
    InjectRegistered(service): InjectRegistered<RecipeService>,
    Path(recipe_id): Path<RecipeId>,
    Query(options): Query<StartRecipeQuery>,
) -> Result<(), (StatusCode, String)> {
    service
        .start_recipe_with(recipe_id, options.pending_changes)
        .await
        .map_err(|e| {
            let status = match &e {
                TransactionError::Other(e) if e.is::<UncommittedChangesError>() => {
                    StatusCode::CONFLICT
                }
                _ => StatusCode::BAD_REQUEST,
            };
            (status, e.to_string())
        })
}
