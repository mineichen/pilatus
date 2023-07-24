use futures::FutureExt;
use minfac::ServiceCollection;
use pilatus::{RecipeExporter, RecipeId};
use pilatus_axum::{
    extract::{InjectRegistered, Path},
    http::StatusCode,
    AppendHeaders, IntoResponse, IoStreamBody, ServiceCollectionExtensions,
};

use crate::zip_writer_wrapper::ZipWriterWrapper;

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("recipe", |r| r
        .http("/:id/export",|m| m.get(export_recipe))
    );
}
async fn export_recipe(
    Path(recipe_id): Path<RecipeId>,
    InjectRegistered(service): InjectRegistered<RecipeExporter>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok((
        AppendHeaders([(
            "Content-Disposition",
            format!("attachment; filename=\"{recipe_id}.pilatusrecipe\""),
        )]),
        IoStreamBody::with_writer(move |w| {
            async move {
                service
                    .export(recipe_id, ZipWriterWrapper::new_boxed(w))
                    .await
            }
            .fuse()
        }),
    ))
}
