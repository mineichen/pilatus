use std::{any::Any, sync::Arc};

use futures::Future;
use minfac::{Resolvable, ServiceBuilder};

use super::{DeviceContext, DeviceHandler, DeviceResult, ValidatorClosure};

pub trait ServiceBuilderExtensions {
    type Dependency: Send;

    fn register_device<
        TFut,
        TParams: Any + Send + Sync,
        TValidator: for<'a> ValidatorClosure<'a, TParams> + Send + Sync + 'static,
    >(
        &mut self,
        device_type: &'static str,
        validator: TValidator,
        handler: fn(DeviceContext, TParams, Self::Dependency) -> TFut,
    ) where
        TFut: Future<Output = DeviceResult> + Send + 'static;
}

impl<'col, TDep> ServiceBuilderExtensions for ServiceBuilder<'col, TDep>
where
    TDep: Resolvable + Send + 'static,
    TDep::ItemPreChecked: Send,
{
    type Dependency = TDep::ItemPreChecked;

    fn register_device<
        TFut,
        TParams: Any + Send + Sync,
        TValidator: for<'a> ValidatorClosure<'a, TParams> + Send + Sync + 'static,
    >(
        &mut self,
        device_type: &'static str,
        validator: TValidator,
        handler: fn(DeviceContext, TParams, Self::Dependency) -> TFut,
    ) where
        TFut: Future<Output = DeviceResult> + Send + 'static,
    {
        self.0.register_instance(
            Box::new(super::DepDeviceHandler::<TDep, TFut, TParams>::new(
                device_type,
                Arc::new(validator),
                handler,
            )) as Box<dyn DeviceHandler>,
        );
    }
}
