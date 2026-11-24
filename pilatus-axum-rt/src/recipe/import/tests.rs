use futures::{Future, TryFutureExt};
use pilatus_rt::RecipeServiceImpl;

use pilatus::{device::DeviceId, DeviceConfig, RecipeExporterTrait, RecipeId, RecipeServiceFacade};
use testresult::TestResult;
use tokio::io::DuplexStream;
use tokio_util::compat::Compat;

use crate::zip_writer_wrapper::ZipWriterWrapper;

mod conflicting_device_after_import;
mod duplicate_self_allowed;
mod replace_self_allowed;
mod replace_without_files;
mod success_replace;
mod with_variables;

async fn build_zip(
    recipe_id: RecipeId,
    device_id: DeviceId,
    device_config: DeviceConfig,
    files: &[(&'static str, &'static str)],
) -> TestResult<Vec<u8>> {
    let (_dir, rsb) = RecipeServiceImpl::create_temp_builder();
    let rs = rsb.build();
    let active_recipe_id = {
        let active_recipe_id = rs.get_active_id().await;
        if recipe_id == active_recipe_id {
            active_recipe_id
        } else {
            rs.add_recipe_with_id(recipe_id.clone(), Default::default())
                .await?;
            rs.start_recipe(recipe_id.clone(), Default::default())
                .await?;
            rs.delete_recipe(active_recipe_id, Default::default())
                .await?;
            recipe_id
        }
    };

    rs.add_device_with_id(active_recipe_id.clone(), device_id, device_config)
        .await?;
    for (path, content) in files {
        rs.create_device_file(device_id, path, content.as_bytes())
            .await;
    }

    writer_into_vec_unchecked(move |w| {
        let rs = rs;
        async move { Ok(rs.export(active_recipe_id, w).await?) }
    })
    .await
}

pub(super) async fn writer_into_vec_unchecked<TRes: Future<Output = TestResult<()>>>(
    x: impl FnOnce(Box<ZipWriterWrapper<Compat<DuplexStream>>>) -> TRes,
) -> TestResult<Vec<u8>> {
    let (mut r, w) = tokio::io::duplex(64);
    let boxed = ZipWriterWrapper::new_boxed(
        tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(w),
    );
    let mut data = Vec::new();
    futures::future::try_join(
        (x)(boxed),
        tokio::io::AsyncReadExt::read_to_end(&mut r, &mut data).map_err(Into::into),
    )
    .await?;

    Ok(data)
}
