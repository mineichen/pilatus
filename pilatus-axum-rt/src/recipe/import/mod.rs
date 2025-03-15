use std::collections::HashSet;

use axum::{
    extract::ws::{Message, WebSocket},
    response::IntoResponse,
};
use minfac::ServiceCollection;
use pilatus::{ImportRecipeError, IntoMergeStrategy, RecipeId, RecipeImporter, VariableConflict};
use pilatus_axum::{
    extract::{ws::WebSocketUpgrade, InjectRegistered},
    ServiceCollectionExtensions,
};
use tracing::{debug, error, info, warn};

use websocket_reader::AsyncWebsocketReader;
use zip_reader_wrapper::ZipReaderWrapper;

use super::zip_to_io_error;

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("recipe", |r| r
        .http("/import",|m| m.get(import_recipes))
    );
}

#[cfg(test)]
mod tests;
mod websocket_reader;
mod zip_reader_wrapper;

async fn import_recipes(
    InjectRegistered(service): InjectRegistered<RecipeImporter>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |s| async {
        if let Err(e) = import_recipes_upgraded(s, service).await {
            debug!("Error during upload: {e}")
        }
    })
}

async fn import_recipes_upgraded(
    mut socket: WebSocket,
    service: RecipeImporter,
) -> Result<(), axum::Error> {
    let mut result = service
        .import(
            &mut ZipReaderWrapper::new(futures_lite::io::BufReader::new(
                tokio_util::compat::TokioAsyncReadCompatExt::compat(AsyncWebsocketReader::new(
                    &mut socket,
                )),
            )),
            Default::default(),
        )
        .await;

    async fn abort_import(socket: &mut WebSocket, msg: String) -> Result<(), axum::Error> {
        debug!(msg);
        socket
            .send(ImportServerMessage::Error(msg).into_message())
            .await?;
        Ok(())
    }

    while let Err(error) = result {
        match error {
            ImportRecipeError::ContainsActiveRecipe => {
                return abort_import(&mut socket, "Import contains active recipe.".to_string())
                    .await;
            }
            ImportRecipeError::InvalidFormat(msg) => {
                return abort_import(&mut socket, format!("Invalid format: {msg}")).await;
            }
            ImportRecipeError::Io(e) => {
                return abort_import(
                    &mut socket,
                    format!("Something went wrong during import. Try again: {e}"),
                )
                .await;
            }
            ImportRecipeError::ExistingDeviceInOtherRecipe(did, rid1, rid2) => {
                return abort_import(
                    &mut socket,
                    format!("{did:?} was contained in both {rid1:?} and {rid2:?}"),
                )
                .await;
            }
            ImportRecipeError::Conflicts(conflicting_ids, var_conflicts, importer) => {
                debug!("Cannot import recipes due to conflicting recipe_ids: {conflicting_ids:?}. Asking for other strategy");
                socket
                    .send(
                        ImportServerMessage::Conflicts(conflicting_ids, var_conflicts)
                            .into_message(),
                    )
                    .await?;

                let strategy = loop {
                    let Some(r) = socket.recv().await else {
                        info!("Recipe import aborted by client");
                        return Ok(());
                    };
                    let r = r?;
                    if let Message::Text(x) = &r {
                        if let Ok(s) = serde_json::from_str::<IntoMergeStrategy>(x) {
                            break s;
                        }
                    };
                    warn!("Invalid message. Expected IntoMergeStrategy, got '{r:?}'");
                };

                result = importer.apply(strategy).await;
            }
            ImportRecipeError::Irreversible(e) => {
                let msg = format!(
                    "Something went terribly wrong during import. Recipes might be corrupted. {e}."
                );
                error!(msg);
                socket
                    .send(ImportServerMessage::Error(msg).into_message())
                    .await?;

                return Ok(());
            }
        }
    }
    socket
        .send(ImportServerMessage::Success.into_message())
        .await?;

    Ok(())
}

#[derive(Debug, serde::Serialize)]
enum ImportServerMessage {
    Success,
    Error(String),
    Conflicts(HashSet<RecipeId>, Vec<VariableConflict>),
}

impl ImportServerMessage {
    fn into_message(self) -> Message {
        Message::Text(serde_json::to_string(&self).unwrap().into())
    }
}
