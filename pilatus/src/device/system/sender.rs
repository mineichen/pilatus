use std::{any::TypeId, borrow::Cow, fmt::Debug, marker::PhantomData, sync::Weak};

use futures::channel::oneshot;

use super::{
    ActorError, ActorErrorBusy, ActorMessage, ActorResult, BoxMessage, InternalSender,
    MessageWithResponse,
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
    pub fn tell(&mut self, msg: TMsg) {
        self.actor_message_sender.tell(msg)
    }
    pub async fn ask(&mut self, msg: TMsg) -> ActorResult<TMsg> {
        self.actor_message_sender.ask(msg).await
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
    // Todo: Should return Result in case MPSC is not available
    pub fn tell<TMsg: ActorMessage>(&mut self, msg: TMsg) {
        let _ignore = self.get_channel(msg);
    }

    pub async fn ask<TMsg: ActorMessage>(&mut self, msg: TMsg) -> ActorResult<TMsg> {
        match self.get_channel(msg)?.await {
            Ok(x) => x,
            Err(_) => Err(ActorError::UnknownMessageType(std::any::type_name::<TMsg>())),
        }
    }

    #[allow(clippy::type_complexity)]
    fn get_channel<TMsg: ActorMessage>(
        &mut self,
        msg: TMsg,
    ) -> Result<oneshot::Receiver<ActorResult<TMsg>>, ActorError<TMsg::Error>> {
        let (tx, rx) = oneshot::channel();

        if self
            .mpsc_sender
            .try_send((
                TypeId::of::<TMsg>(),
                BoxMessage(Box::new(MessageWithResponse::new(msg, tx))),
            ))
            .is_err()
        {
            return Err(ActorError::Busy(ActorErrorBusy::ExceededQueueCapacity(
                self.device_id,
            )));
        }
        Ok(rx)
    }
}

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
    pub fn tell<TMsg: ActorMessage>(&mut self, msg: TMsg) {
        if let Ok(mut x) = self.build_strong::<TMsg>() {
            x.tell(msg);
        }
    }

    pub async fn ask<TMsg: ActorMessage>(&mut self, msg: TMsg) -> ActorResult<TMsg> {
        self.build_strong::<TMsg>()?.ask(msg).await
    }

    fn build_strong<TMsg: ActorMessage>(
        &self,
    ) -> Result<UntypedActorMessageSender, ActorError<TMsg::Error>> {
        let mpsc_sender = InternalSender::clone(
            self.mpsc_sender
                .upgrade()
                .ok_or(ActorErrorUnknownDevice {
                    device_id: self.device_id,
                    detail: Cow::Borrowed(
                        "Channel from WeakUntypedActorMessageSender was dropped already",
                    ),
                })?
                .as_ref(),
        );

        Ok(UntypedActorMessageSender::new(self.device_id, mpsc_sender))
    }
}
