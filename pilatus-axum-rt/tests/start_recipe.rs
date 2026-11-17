#![cfg(feature = "integration")]
use std::{sync::Arc, time::Duration};

use minfac::Registered;
use pilatus::{DeviceConfig, Recipe, RecipeId, Recipes};
use pilatus_rt::{RecipeServiceImpl, TempRuntime};
use reqwest::{redirect::Policy, StatusCode};

async fn get_all(base: &str, client: &reqwest::Client) -> anyhow::Result<serde_json::Value> {
    Ok(client
        .get(format!("{base}/recipe/get_all"))
        .send()
        .await?
        .json()
        .await?)
}

async fn active_id(base: &str, client: &reqwest::Client) -> anyhow::Result<RecipeId> {
    let state = get_all(base, client).await?;
    Ok(serde_json::from_value(state["active_id"].clone())?)
}

/// The runner needs a moment after startup or a recipe change, until it accepts the next request
async fn put_start(client: &reqwest::Client, url: &str) -> anyhow::Result<(StatusCode, String)> {
    for _ in 0..100 {
        let response = client.put(url).send().await?;
        let status = response.status();
        let text = response.text().await?;
        if !text.contains("Not ready") {
            return Ok((status, text));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    anyhow::bail!("Runner never got ready")
}

#[test]
fn uncommitted_changes_are_a_conflict() -> anyhow::Result<()> {
    let mut recipes = Recipes::new_with_recipe(Recipe::default());
    let other_id = recipes.add_new(Recipe::default());

    TempRuntime::new()
        .config(serde_json::json!({ "web": { "socket": "0.0.0.0:0" } }))
        .set_recipes(recipes)
        .register(pilatus_axum_rt::register)
        .configure()?
        .run_until(
            |(Registered(web_stats), Registered(rs)): (
                Registered<pilatus_axum::Stats>,
                Registered<Arc<RecipeServiceImpl>>,
            )| async move {
                let port = web_stats.socket_addr().await.port();
                let base = format!("http://127.0.0.1:{port}/api");
                let client = reqwest::Client::new();
                let initial_id = active_id(&base, &client).await?;
                let device_id = rs
                    .add_device_to_active_recipe(DeviceConfig::mock("params"))
                    .await?;
                let device = |state: &serde_json::Value| {
                    state["all"][initial_id.to_string()]["devices"][device_id.to_string()].clone()
                };
                assert_ne!(
                    serde_json::Value::Null,
                    device(&get_all(&base, &client).await?)
                );

                let start_url = format!("{base}/recipe/start/{other_id}");
                let (status, text) = put_start(&client, &start_url).await?;
                assert_eq!(StatusCode::CONFLICT, status, "{text}");
                assert_eq!("Other: Uncommitted changes on active recipe", text);
                assert_eq!(initial_id, active_id(&base, &client).await?);

                let (status, text) =
                    put_start(&client, &format!("{start_url}?pending_changes=discard")).await?;
                assert_eq!(StatusCode::OK, status, "{text}");
                assert_eq!(other_id, active_id(&base, &client).await?);

                assert_eq!(
                    serde_json::Value::Null,
                    device(&get_all(&base, &client).await?)
                );
                Ok(())
            },
        )
}

#[test]
fn start_recipe_is_only_possible_with_put() -> anyhow::Result<()> {
    let mut recipes = Recipes::new_with_recipe(Recipe::default());
    let other_id = recipes.add_new(Recipe::default());

    TempRuntime::new()
        .config(serde_json::json!({ "web": { "socket": "0.0.0.0:0" } }))
        .set_recipes(recipes)
        .register(pilatus_axum_rt::register)
        .configure()?
        .run_until(
            |Registered(web_stats): Registered<pilatus_axum::Stats>| async move {
                let port = web_stats.socket_addr().await.port();
                let base = format!("http://127.0.0.1:{port}/api");
                let start_url = format!("{base}/recipe/start/{other_id}");
                let client = reqwest::Client::builder()
                    .redirect(Policy::none())
                    .build()?;
                let initial_id = active_id(&base, &client).await?;
                assert_ne!(initial_id, other_id);

                // Discarding must not be reachable by a simple link
                for query in ["", "?pending_changes=discard", "?pending_changes=commit"] {
                    let response = client.get(format!("{start_url}{query}")).send().await?;
                    assert_eq!(StatusCode::MOVED_PERMANENTLY, response.status());
                }
                assert_eq!(initial_id, active_id(&base, &client).await?);

                // The runner needs a moment after startup, until it accepts the first request
                let mut attempts = 0;
                let response = loop {
                    let response = client.put(&start_url).send().await?;
                    attempts += 1;
                    if response.status().is_success() || attempts > 100 {
                        break response;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                };
                assert_eq!(StatusCode::OK, response.status());
                assert_eq!(other_id, active_id(&base, &client).await?);
                Ok(())
            },
        )
}
