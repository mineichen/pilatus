//! A `WithAbort` handler can still be aborted after it returned `Step2`.
//! Currently, a request can only be aborted by dropping its future.

use std::pin::pin;
use std::time::Duration;

use futures::future::{select, Abortable, Either};
use futures::stream::AbortRegistration;
use futures::FutureExt;
use minfac::{Registered, ServiceCollection};
use pilatus::device::{
    ActorMessage, ActorResult, ActorSystem, DeviceContext, DeviceResult, DeviceValidationContext,
    HandlerResult, Step2, WithAbort,
};
use pilatus::prelude::*;
use pilatus::{DeviceConfig, Recipe, UpdateParamsMessageError};
use pilatus_rt::TempRuntime;
use testresult::TestResult;
use tokio::sync::Semaphore;

static STEP2_STARTED: Semaphore = Semaphore::const_new(0);
static STEP2_ABORTED: Semaphore = Semaphore::const_new(0);

#[test]
fn dropping_the_request_aborts_step2() -> TestResult {
    let mut recipe = Recipe::default();
    let device_id = recipe.add_device(DeviceConfig::new_unchecked("waiter", "Waiter", ()));

    TempRuntime::new()
        .add_recipe(recipe)
        .register(register_waiter)
        .configure()?
        .run_until(
            |Registered(actor_system): Registered<ActorSystem>| async move {
                {
                    // Boxed, so dropping `request` drops the future
                    let request = pin!(actor_system.ask(device_id, WaitForAbort));
                    let started = pin!(wait_for_permit(&STEP2_STARTED));

                    let Either::Right((started, request)) = select(request, started).await else {
                        return Err("The request finished before Step2 started".into());
                    };
                    started?;
                }

                wait_for_permit(&STEP2_ABORTED).await
            },
        )
}

async fn wait_for_permit(semaphore: &Semaphore) -> TestResult {
    tokio::time::timeout(Duration::from_secs(1), semaphore.acquire())
        .await??
        .forget();
    Ok(())
}

struct WaitForAbort;

impl ActorMessage for WaitForAbort {
    type Output = ();
    type Error = ();
}

async fn wait_for_abort(
    _state: &mut (),
    _msg: WaitForAbort,
    abort: AbortRegistration,
) -> impl HandlerResult<WaitForAbort> {
    Step2(async move {
        STEP2_STARTED.add_permits(1);
        let _aborted = Abortable::new(std::future::pending::<()>(), abort).await;
        STEP2_ABORTED.add_permits(1);
        ActorResult::<WaitForAbort>::Ok(())
    })
}

extern "C" fn register_waiter(c: &mut ServiceCollection) {
    async fn validator(ctx: DeviceValidationContext<'_>) -> Result<(), UpdateParamsMessageError> {
        ctx.params_as()
    }

    async fn device(ctx: DeviceContext, _params: (), actor_system: ActorSystem) -> DeviceResult {
        actor_system
            .register(ctx.id)
            .add_handler(WithAbort::new(wait_for_abort))
            .execute(())
            .await;
        Ok(())
    }

    c.with::<Registered<ActorSystem>>()
        .register_device("waiter", validator, device);
}
