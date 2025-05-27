use std::fmt::Debug;
use std::time::Duration;

use kameo::message::StreamMessage;
use kameo::prelude::*;
use ryushi_protocol::command::ShardCommand;
use ryushi_protocol::info::ShardInfo;
use serde::{Deserialize, Serialize};
use twilight_gateway::{EventType, MessageSender, ShardId};

use crate::data::DataLayer;

/// An actor that is designed to run alongside the Shard and handle all
/// interactions with the data layer, allowing the event loop to focus on
/// processing events without blocking on application logic.
pub struct Sidecar<DL: DataLayer> {
    /// The data layer used to interact with the storage.
    data_layer: DL,
    /// The ID of the shard this sidecar is associated with.
    id: ShardId,
    /// The sender used to send messages to the shard.
    sender: MessageSender
}

pub type SidecarRef<DL> = ActorRef<Sidecar<DL>>;

impl<DL: DataLayer> Sidecar<DL> {
    pub fn spawn(data_layer: DL, id: ShardId, sender: MessageSender) -> ActorRef<Self> {
        <Self as Actor>::spawn_with_mailbox(Self { data_layer, id, sender }, mailbox::unbounded())
    }
}

impl<DL: DataLayer> Actor for Sidecar<DL> {
    type Args = Self;
    type Error = DL::Error;

    async fn on_start(args: Self, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        actor_ref.attach_stream(args.data_layer.shard_subscribe(args.id).await?, (), ());

        Ok(args)
    }
}

impl<DL: DataLayer> Message<StreamMessage<Result<ShardCommand, DL::Error>, (), ()>>
    for Sidecar<DL>
{
    type Reply = ();

    async fn handle(
        &mut self, msg: StreamMessage<Result<ShardCommand, DL::Error>, (), ()>,
        _: &mut Context<Self, Self::Reply>
    ) -> Self::Reply {
        let StreamMessage::Next(command) = msg else {
            return;
        };

        tracing::trace!(shard_id=?self.id, ?command, "== processing shard command");
        match command {
            Ok(ShardCommand::Gateway(command)) => match command.encode() {
                Ok(json) => {
                    let _ = self.sender.send(json);
                }
                Err(err) => {
                    tracing::warn!(?err, shard_id=?self.id, "Failed to serialize gateway command");
                }
            },
            Ok(ShardCommand::Reconnect { close_style }) => {
                let _ = self.sender.close(close_style.into());
            }
            Err(err) => {
                tracing::error!(?err, shard_id=?self.id, "Failed to process shard command");
            }
        }
    }
}

impl<DL: DataLayer> Message<ShardInfo> for Sidecar<DL> {
    type Reply = ();

    async fn handle(&mut self, msg: ShardInfo, _: &mut Context<Self, Self::Reply>) -> Self::Reply {
        tracing::trace!(shard_id=?self.id, ?msg, "== actor: updating shard info");
        if let Err(err) = self.data_layer.shard_save(self.id, &msg).await {
            tracing::error!(?err, shard_id=?self.id, "Error writing shard state");
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct HeartbeatRtt(pub Duration);

impl<DL: DataLayer> Message<HeartbeatRtt> for Sidecar<DL> {
    type Reply = ();

    async fn handle(
        &mut self, msg: HeartbeatRtt, _: &mut Context<Self, Self::Reply>
    ) -> Self::Reply {
        tracing::trace!(shard_id=?self.id, ?msg, "== actor: observing heartbeat RTT");

        if let Err(err) = self.data_layer.observe_heartbeat_rtt(self.id, msg.0).await {
            tracing::error!(?err, shard_id=?self.id, "Failed to write shard latency to store");
        };
    }
}

pub struct Dispatch {
    /// The type of the event.
    pub event_type: EventType,
    /// The data of the event.
    pub data: String
}

impl Debug for Dispatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Dispatch").field(&self.event_type).finish()
    }
}

impl<DL: DataLayer> Message<Dispatch> for Sidecar<DL> {
    type Reply = ();

    async fn handle(&mut self, msg: Dispatch, _: &mut Context<Self, Self::Reply>) -> Self::Reply {
        tracing::trace!(shard_id=?self.id, ?msg, "== actor: observing event");

        if let Err(err) = self.data_layer.observe_event(self.id, msg).await {
            tracing::error!(?err, shard_id=?self.id, "Failed to observe event");
        }
    }
}
