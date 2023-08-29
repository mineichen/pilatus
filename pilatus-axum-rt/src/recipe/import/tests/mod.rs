use futures::{future::join, Future};
use pilatus_rt::RecipeServiceFassade;

use pilatus::{device::DeviceId, DeviceConfig, RecipeExporterTrait, RecipeId, RecipeServiceTrait};
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
) -> Vec<u8> {
    let (_dir, rsb) = RecipeServiceFassade::create_temp_builder();
    let rs = rsb.build();
    let active_recipe_id = {
        let active_recipe_id = rs.get_active_id().await;
        if recipe_id == active_recipe_id {
            active_recipe_id
        } else {
            rs.add_recipe_with_id(recipe_id.clone(), Default::default())
                .await
                .unwrap();
            rs.set_recipe_to_active(recipe_id.clone(), Default::default())
                .await
                .unwrap();
            rs.delete_recipe(active_recipe_id).await.unwrap();
            recipe_id
        }
    };

    rs.add_device_with_id(active_recipe_id.clone(), device_id, device_config)
        .await
        .unwrap();
    for (path, content) in files {
        rs.create_device_file(device_id, path, content.as_bytes())
            .await;
    }

    writer_into_vec_unchecked(move |w| {
        let rs = rs;
        async move { rs.export(active_recipe_id, w).await }
    })
    .await
}

pub(super) async fn writer_into_vec_unchecked<TRes: Future<Output = anyhow::Result<()>>>(
    x: impl FnOnce(Box<ZipWriterWrapper<Compat<DuplexStream>>>) -> TRes,
) -> Vec<u8> {
    let (mut r, w) = tokio::io::duplex(64);
    let boxed = ZipWriterWrapper::new_boxed(
        tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(w),
    );
    let mut data = Vec::new();
    let (read, write) = join(
        (x)(boxed),
        tokio::io::AsyncReadExt::read_to_end(&mut r, &mut data),
    )
    .await;
    read.unwrap();
    write.unwrap();

    data
}
