use anyhow::Result;
use sealedstruct::Sealable;
use serde::{de::DeserializeOwned, Serialize};
use tracing::warn;

use crate::{MaybeVar, RawVariable, UntypedDeviceParamsWithVariables, Variables};

use super::{DeviceContext, UpdateParamsMessageError, WithInfallibleParamUpdate};

#[non_exhaustive]
pub struct DeviceValidationContext<'a> {
    pub(super) enable_autorepair: bool,
    pub(super) raw: &'a DeviceContext,
}

impl<'a> DeviceValidationContext<'a> {
    pub fn params_as<T: DeserializeOwned>(&self) -> Result<T, UpdateParamsMessageError> {
        let resolved = self.raw.variables.resolve(&self.raw.params_with_vars)?;
        Ok(resolved.params_as::<T>()?)
    }

    pub fn params_as_sealed<T: DeserializeOwned + Sealable>(
        &self,
    ) -> Result<T::Target, UpdateParamsMessageError>
    where
        T::Target:,
    {
        {
            let resolved = self.raw.variables.resolve(&self.raw.params_with_vars)?;

            resolved
                .params_as::<T>()
                .map_err(Into::into)
                .and_then(|x| x.seal().map_err(Into::into))
        }
    }

    /// Similar to params_as_sealed, but tries to repair the device_params, if `DeviceValidationContext::enable_autorepair`
    /// is enabled.
    /// - For changes which are coming through the UI, it's disabled and it behaves like `params_as_sealed`
    /// - For changes which are coming from Import or Startup, it's enabled. The configuration is loaded and reexported
    ///   If the generated configuration differs from the loaded one, an Error is triggered indicating the need for params-change
    pub fn params_as_sealed_autorepair<T: DeserializeOwned + Sealable + RawVariable>(
        &self,
    ) -> Result<WithInfallibleParamUpdate<T::Target>, UpdateParamsMessageError>
    where
        T::Variable: DeserializeOwned + Serialize,
    {
        match (self.params_as_sealed::<T>(), self.enable_autorepair) {
            (Err(_e), true) => {
                let raw_var: MaybeVar<T::Variable> =
                    self.raw.variables.resolve_var(&self.raw.params_with_vars)?;
                let with_vars = match self.raw.variables.unresolve_var::<T>(&raw_var) {
                    Ok((_, Some(variables))) => {
                        return Err(UpdateParamsMessageError::VariableError(format!(
                            "Changes on variables are not yet supported: {variables:?}"
                        )))
                    }
                    Ok((x, None)) => x,
                    Err(e) => {
                        return Err(UpdateParamsMessageError::VariableError(format!(
                            "Unexpected Variable-Error: Should always be resolvable: {e:?}"
                        )))
                    }
                };

                let raw_value: T = raw_var.into_resolved().into();
                match raw_value.seal() {
                    Ok(data) => {
                        warn!("Successfully migrated parameters ");
                        Ok(WithInfallibleParamUpdate {
                            data,
                            update: Some(with_vars),
                        })
                    }
                    Err(e) => Err(e.into()),
                }
            }
            (x, _) => x.map(|data| WithInfallibleParamUpdate { data, update: None }),
        }
    }

    pub fn unresolved(&self) -> (&Variables, &UntypedDeviceParamsWithVariables) {
        (&self.raw.variables, &self.raw.params_with_vars)
    }
}
