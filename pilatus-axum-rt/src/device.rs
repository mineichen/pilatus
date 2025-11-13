use minfac::ServiceCollection;
use pilatus::{device::RecipeRunner, RecipeId};
use pilatus_axum::{
    extract::{InjectRegistered, Path},
    http::StatusCode,
    ServiceCollectionExtensions,
};

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("recipe", |r| r
        .http("/start/{id}", |m| m.get(set_active).put(set_active))
    );
}

async fn set_active(
    InjectRegistered(runner): InjectRegistered<RecipeRunner>,
    Path(recipe_id): Path<RecipeId>,
) -> Result<(), (StatusCode, String)> {
    runner
        .select_recipe(recipe_id)
        .await
        .map_err(|x| (StatusCode::BAD_REQUEST, x.to_string()))
}
