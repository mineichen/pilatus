use std::{any::TypeId, borrow::Cow, fmt::Debug, future::Future, marker::PhantomData, sync::Weak};

use futures_channel::oneshot;

use super::{
    ActorError, ActorErrorBusy, ActorMessage, ActorRequestResponder, ActorResult,
    ActorWeakTellError, BoxMessage, InternalSender, MessageWithResponder,
};
use crate::{device::ActorErrorUnknownDevice, device::DeviceId};

#[derive(Debug)]
pub struct UntypedActorMessageSender {
    device_id: DeviceId,
    mpsc_sender: InternalSender,
}

pub struct ActorMessageSender<T> {
    actor_message_sender: UntypedActorMessageSender,
    phantom: PhantomData<T>,
}

impl<TMsg: ActorMessage> ActorMessageSender<TMsg> {
    pub fn new(actor_message_sender: UntypedActorMessageSender) -> Self {
        ActorMessageSender {
            actor_message_sender,
            phantom: PhantomData,
        }
    }
    pub fn tell(&mut self, msg: TMsg) -> Result<(), ActorErrorBusy> {
        self.actor_message_sender.tell(msg)
    }
    /// See [`UntypedActorMessageSender::ask`]
    pub fn ask(&mut self, msg: TMsg) -> impl Future<Output = ActorResult<TMsg>> + Send + 'static {
        self.actor_message_sender.ask(msg)
    }
}

impl UntypedActorMessageSender {
    pub(super) fn new(device_id: DeviceId, mpsc_sender: InternalSender) -> Self {
        Self {
            device_id,
            mpsc_sender,
        }
    }

    /// Sends a message without awaiting a response. It's error-handling is therefore limited to see whether the Target-Actor accepts the message in it's queue
    pub fn tell<TMsg: ActorMessage>(&mut self, msg: TMsg) -> Result<(), ActorErrorBusy> {
        let _ignore = self.get_channel(msg)?;
        Ok(())
    }

    /// The message is sent immediately (like [`UntypedActorMessageSender::tell`]), the future only waits for the response.
    /// This allows the future to be `Send + 'static` independent of `TMsg` without cloning the sender
    /// (see [`crate::device::ActorSystem::ask`])
    pub fn ask<TMsg: ActorMessage>(
        &mut self,
        msg: TMsg,
    ) -> impl Future<Output = ActorResult<TMsg>> + Send + 'static {
        let channel = self.get_channel(msg);
        async move {
            match channel?.await {
                Ok(x) => x,
                Err(_) => Err(ActorError::UnknownMessageType(std::any::type_name::<TMsg>())),
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn get_channel<TMsg: ActorMessage>(
        &mut self,
        msg: TMsg,
    ) -> Result<oneshot::Receiver<ActorResult<TMsg>>, ActorErrorBusy> {
        let (tx, rx) = oneshot::channel();
        let responder = ActorRequestResponder::new(tx);
        if self
            .mpsc_sender
            .try_send((
                TypeId::of::<TMsg>(),
                BoxMessage(Box::new(MessageWithResponder::new(msg, responder))),
            ))
            .is_err()
        {
            return Err(ActorErrorBusy::ExceededQueueCapacity(self.device_id));
        }
        Ok(rx)
    }
}

#[derive(Clone)]
pub struct WeakUntypedActorMessageSender {
    device_id: DeviceId,
    mpsc_sender: Weak<InternalSender>,
}

impl WeakUntypedActorMessageSender {
    pub fn new(device_id: DeviceId, mpsc_sender: Weak<InternalSender>) -> Self {
        Self {
            device_id,
            mpsc_sender,
        }
    }

    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    pub fn tell<TMsg: ActorMessage>(&mut self, msg: TMsg) -> Result<(), ActorWeakTellError> {
        if let Ok(mut x) = self.build_strong::<TMsg>() {
            Ok(x.tell(msg)?)
        } else {
            Err(ActorWeakTellError::UnknownDevice(
                ActorErrorUnknownDevice::UnknownDeviceId {
                    device_id: self.device_id,
                    details: "Device existed but is no longer available".into(),
                },
            ))
        }
    }

    /// The device is looked up immediately, the message is sent when the future is polled
    /// (see [`crate::device::ActorSystem::ask`])
    pub fn ask<TMsg: ActorMessage>(
        &mut self,
        msg: TMsg,
    ) -> impl Future<Output = ActorResult<TMsg>> + Send + 'static {
        let sender = self.build_strong::<TMsg>();
        async move { sender?.ask(msg).await }
    }

    fn build_strong<TMsg: ActorMessage>(
        &self,
    ) -> Result<UntypedActorMessageSender, ActorError<TMsg::Error>> {
        let mpsc_sender = InternalSender::clone(
            self.mpsc_sender
                .upgrade()
                .ok_or(ActorErrorUnknownDevice::UnknownDeviceId {
                    device_id: self.device_id,
                    details: Cow::Borrowed(
                        "Channel from WeakUntypedActorMessageSender was dropped already",
                    ),
                })?
                .as_ref(),
        );

        Ok(UntypedActorMessageSender::new(self.device_id, mpsc_sender))
    }
}
