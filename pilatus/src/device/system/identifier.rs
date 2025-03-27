use std::any::TypeId;

use futures_channel::mpsc;
use serde::Deserialize;

use super::{
    ActorErrorUnknownDevice, ActorMessage, ActorMessageSender, ActorSystemState,
    UntypedActorMessageSender,
};
use crate::device::DeviceId;

pub struct SealedActorSystemState<'a>(pub(super) &'a ActorSystemState);

pub trait ActorSystemIdentifier: Sized {
    fn get_untyped_sender(
        self,
        actor_system: SealedActorSystemState,
    ) -> Result<UntypedActorMessageSender, ActorErrorUnknownDevice>;
    fn get_typed_sender<TMsg: ActorMessage>(
        self,
        actor_system: SealedActorSystemState,
    ) -> Result<ActorMessageSender<TMsg>, ActorErrorUnknownDevice> {
        self.get_untyped_sender(actor_system)
            .map(ActorMessageSender::new)
    }
}

impl ActorSystemIdentifier for DeviceId {
    fn get_untyped_sender(
        self,
        state: SealedActorSystemState,
    ) -> Result<UntypedActorMessageSender, ActorErrorUnknownDevice> {
        let mpsc_sender = state
            .0
            .devices
            .get(&self)
            .map(|x| mpsc::Sender::clone(x))
            .ok_or(ActorErrorUnknownDevice::UnknownDeviceId {
                device_id: self,
                details: "No message queue for this device".into(),
            })?;
        Ok(UntypedActorMessageSender::new(self, mpsc_sender))
    }
}

impl ActorSystemIdentifier for DynamicIdentifier {
    fn get_untyped_sender(
        self,
        actor_system: SealedActorSystemState,
    ) -> Result<UntypedActorMessageSender, ActorErrorUnknownDevice> {
        match self {
            DynamicIdentifier::DeviceId(device_id) => device_id.get_untyped_sender(actor_system),
            DynamicIdentifier::None => todo!(),
        }
    }
    fn get_typed_sender<TMsg: ActorMessage>(
        self,
        actor_system: SealedActorSystemState,
    ) -> Result<ActorMessageSender<TMsg>, ActorErrorUnknownDevice> {
        match self {
            DynamicIdentifier::DeviceId(device_id) => device_id.get_typed_sender(actor_system),
            DynamicIdentifier::None => {
                let ids = actor_system.0.messages.get(&TypeId::of::<TMsg>());
                let mut ids_iter = ids.iter().flat_map(|x| x.iter());
                let Some(id) = ids_iter.next() else {
                    return Err(ActorErrorUnknownDevice::AmbiguousHandler {
                        msg_type: std::any::type_name::<TMsg>(),
                        possibilities: Default::default(),
                    });
                };

                if ids_iter.next().is_none() {
                    (*id).get_typed_sender(actor_system)
                } else {
                    Err(ActorErrorUnknownDevice::AmbiguousHandler {
                        msg_type: std::any::type_name::<TMsg>(),
                        possibilities: ids.iter().flat_map(|x| x.iter()).copied().collect(),
                    })
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DynamicIdentifier {
    DeviceId(DeviceId),
    None,
}

impl<'de> Deserialize<'de> for DynamicIdentifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct DeDynamicIdentifier {
            device_id: Option<DeviceId>,
        }
        let x = DeDynamicIdentifier::deserialize(deserializer)?;
        Ok(match x.device_id {
            Some(x) => DynamicIdentifier::DeviceId(x),
            None => DynamicIdentifier::None,
        })
    }
}

impl Default for DynamicIdentifier {
    fn default() -> Self {
        Self::None
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn deserialize_dynamic_none() {
        let serde = serde_json::json!({});
        let id = DynamicIdentifier::deserialize(serde).unwrap();
        assert_eq!(id, DynamicIdentifier::None);
    }
    #[test]
    fn deserialize_device_id() {
        let device_id = DeviceId::new_v4();
        let serde = serde_json::json!({"device_id": device_id});
        let id = DynamicIdentifier::deserialize(serde).unwrap();
        assert_eq!(id, DynamicIdentifier::DeviceId(device_id));
    }
}
