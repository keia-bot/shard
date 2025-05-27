use chrono::Utc;
use futures_util::StreamExt;
use ryushi_protocol::command::CloseStyle;
use ryushi_protocol::info::{Heartbeat, ShardInfo};
use serde::de::DeserializeSeed;
use serde_json::de::Deserializer;
use tokio::sync::oneshot;
use twilight_gateway::error::ReceiveMessageError;
use twilight_gateway::queue::Queue;
use twilight_gateway::{EventTypeFlags, Message, Shard};
use twilight_model::gateway::OpCode;
use twilight_model::gateway::event::{
    DispatchEvent, EventType, GatewayEvent, GatewayEventDeserializer
};
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::data::DataLayer;
use crate::event_handler::EventHandler;
use crate::sidecar::{Dispatch, HeartbeatRtt, SidecarRef};

pub struct EventLoop<Q, H, DL>
where
    Q: Queue + Unpin,
    H: EventHandler,
    DL: DataLayer
{
    /// The types of events that the event loop should send to the sidecar.
    events: EventTypeFlags,
    /// The shard that this event loop will handle events for.
    shard: Shard<Q>,
    /// A reference to the sidecar that will be used to send events and metrics.
    sidecar: SidecarRef<DL>,
    /// An event handler that will process events received from the shard.
    event_tx: H
}

struct State {
    shutting_down: bool,
    initial_guilds_remaining: usize,
    info: ShardInfo,
    info_changed: bool,
    initial_guilds: Vec<Id<GuildMarker>>
}

impl<Q, H, S> EventLoop<Q, H, S>
where
    Q: Queue + Unpin,
    H: EventHandler,
    S: DataLayer
{
    #[must_use]
    pub const fn new(
        events: EventTypeFlags, event_tx: H, sidecar: SidecarRef<S>, shard: Shard<Q>
    ) -> Self {
        Self { events, shard, sidecar, event_tx }
    }

    /// # Panics
    ///
    /// If the shard could not subscribe to the command stream.
    pub async fn run(mut self, info: ShardInfo, mut rx_close: oneshot::Receiver<CloseStyle>) {
        let mut state = State {
            shutting_down: false,
            initial_guilds_remaining: 0,
            info,
            info_changed: false,
            initial_guilds: Vec::with_capacity(0)
        };

        // update the shard's state with the initial info.
        state.info.state = self.shard.state();

        state.info.full_ready = false;

        self.sidecar
            .tell(state.info.clone())
            .try_send()
            .expect("Actor channel is closed");

        // start reading messages from the shard.
        let mut close_style = None;
        loop {
            tokio::select! {
                biased;

                // check for a close signal.
                Ok(style) = &mut rx_close => {
                    close_style = Some(style);
                    break;
                }

                // check for an event from the shard.
                item = self.shard.next() => {
                    if self.handle_message(&mut state, item) {
                        break;
                    }
                }
            }

            // flush any info changes to the sidecar.
            if state.info_changed {
                self.sidecar
                    .tell(state.info.clone())
                    .try_send()
                    .expect("Actor channel is closed");

                state.info_changed = false;
            }
        }

        // if we received a close signal, close the shard.
        if let Some(style) = close_style {
            tracing::info!(shard_id=?self.shard.id(), ?style, "Shutting down shard");

            state.shutting_down = true;
            self.shard.close(style.into());

            // keep reading messages until the shard is closed.
            loop {
                let item: Option<Result<Message, ReceiveMessageError>> = self.shard.next().await;
                if self.handle_message(&mut state, item) {
                    break;
                }
            }
        }

        state.info.state = self.shard.state();

        // flush any remaining info changes to the sidecar.
        self.sidecar
            .tell(state.info)
            .try_send()
            .expect("Actor channel is closed");

        // stop the sidecar.
        self.sidecar
            .stop_gracefully()
            .await
            .expect("Actor task did not stop gracefully");

        self.sidecar.wait_for_shutdown().await;
    }

    fn handle_message(
        &self, state: &mut State, item: Option<Result<Message, ReceiveMessageError>>
    ) -> bool {
        // check for a change in the shard's state. this is a bit scuffed but it works.
        let shard_state = self.shard.state();
        if shard_state != state.info.state {
            state.info.state = shard_state;
            state.info_changed = true;
        }

        let message = match item {
            Some(Ok(message)) => message,
            Some(Err(err)) => {
                tracing::error!(
                    ?err,
                    shard_id=?self.shard.id(),
                    "An error occurred while processing the next shard message",
                );

                if let Err(err) = self.event_tx.on_error(err) {
                    tracing::error!(?err, shard_id=?self.shard.id(), "Event handler failed to handle error");
                }

                return false;
            }

            // if the stream returns None, the shard died.
            None => {
                tracing::warn!(shard_id=?self.shard.id(), "Shard stream ended");
                return true;
            }
        };

        let json = match message {
            Message::Close(frame) => {
                tracing::warn!(
                    shard_id=?self.shard.id(),
                    ?frame,
                    reconnecting = !state.shutting_down,
                    "Received close frame from websocket.",
                );

                // update the state.
                state.info.session = self.shard.session().cloned();

                state.info.resume_url = self.shard.resume_url().map(ToString::to_string);

                state.info.heartbeat = None;

                state.info_changed = true;

                if let Err(err) = self.event_tx.on_close(frame) {
                    tracing::error!(?err, shard_id=?self.shard.id(), "Event handler failed to handle close frame");
                }

                // the return value of handle_message determines whether to stop the event loop.
                return state.shutting_down;
            }

            Message::Text(json) => json
        };

        let Some(deserializer) = GatewayEventDeserializer::from_json(&json) else {
            tracing::warn!(shard_id=?self.shard.id(), "Received invalid gateway event: {}", json);
            return false;
        };

        tracing::trace!(shard_id=?self.shard.id(), op=?deserializer.op(), event_type=?deserializer.event_type(), "Received gateway event");
        match OpCode::from(deserializer.op()) {
            Some(OpCode::Dispatch) => {
                let Some(event_type) = deserializer
                    .event_type()
                    .and_then(|it| EventType::try_from(it).ok())
                else {
                    tracing::trace!(shard_id=?self.shard.id(), "Received dispatch event with no type: {}", json);
                    return false;
                };

                let mut json_deserializer = Deserializer::from_str(&json);

                let event = match deserializer.deserialize(&mut json_deserializer) {
                    Ok(GatewayEvent::Dispatch(_, event)) => event,
                    Ok(event) => {
                        tracing::warn!(
                            shard_id=?self.shard.id(),
                            "Received dispatch event that wasn't a dispatch event: {:?}",
                            event
                        );

                        return false;
                    }
                    Err(err) => {
                        tracing::warn!(shard_id=?self.shard.id(), ?err, "Failed to deserialize dispatch event");
                        return false;
                    }
                };

                let is_full_ready = state.info.full_ready;
                match &event {
                    DispatchEvent::Ready(ready) => {
                        state.initial_guilds = ready.guilds.iter().map(|g| g.id).collect();

                        state.info.full_ready = false;

                        state.info.session = self.shard.session().cloned();

                        state.info.ready_at = Some(Utc::now());

                        state.info.resume_url = Some(ready.resume_gateway_url.clone());

                        let guilds = ready.guilds.len();
                        state.initial_guilds_remaining = guilds;

                        if guilds < u16::MAX as usize {
                            // although it should never happen, we don't want to panic incase of a
                            // bug.
                            state.info.guild_count = guilds as u16;
                        }

                        state.info_changed = true;
                        tracing::info!(shard_id=?self.shard.id(), "Shard is ready with {} guilds", guilds);
                    }
                    DispatchEvent::Resumed => {
                        state.info.full_ready = true;
                        state.info_changed = true;
                        tracing::info!(shard_id=?self.shard.id(), "Shard resumed");
                    }
                    DispatchEvent::GuildCreate(event) => {
                        // don't count the initial guilds
                        if state.initial_guilds.contains(&event.id()) {
                            state.initial_guilds_remaining =
                                state.initial_guilds_remaining.saturating_sub(1);

                            state.info.full_ready = state.initial_guilds_remaining == 0;
                        } else {
                            // although it should never happen, use saturating operations to prevent
                            // panics
                            state.info.guild_count = state.info.guild_count.saturating_add(1);
                            state.info_changed = true;
                        }
                    }
                    DispatchEvent::GuildDelete(event) => {
                        // although it should never happen, use saturating operations to prevent
                        // panics
                        state.info.guild_count = state.info.guild_count.saturating_sub(1);
                        state.info_changed = true;

                        if state.initial_guilds.contains(&event.id) {
                            state.initial_guilds_remaining =
                                state.initial_guilds_remaining.saturating_sub(1);

                            state.info.full_ready = state.initial_guilds_remaining == 0;
                        }
                    }
                    _ => {}
                }

                if !is_full_ready && state.info.full_ready {
                    state.info_changed = true;
                    tracing::info!(shard_id=?self.shard.id(), "Shard has received all initial guilds!");
                }

                // offload the event to the consumer and if required, to the sidecar for the
                // application to process.

                // send event to a consumer, e.g., a cache.
                if let Err(err) = self.event_tx.on_dispatch(event) {
                    tracing::error!(?err, shard_id=?self.shard.id(), "Failed to send event to consumer");
                }

                if self.events.contains(event_type.into()) {
                    // send the event to the sidecar so it can be observed.
                    self.sidecar
                        .tell(Dispatch { event_type, data: json })
                        .try_send()
                        .expect("Actor channel is closed");
                }
            }

            Some(OpCode::HeartbeatAck) => {
                let Some(latest) = self.shard.latency().recent().first().cloned() else {
                    return false;
                };

                state.info.heartbeat = Some(Heartbeat { at: Utc::now(), rtt: latest });

                state.info_changed = true;

                self.sidecar
                    .tell(HeartbeatRtt(latest))
                    .try_send()
                    .expect("Actor channel is closed");
            }

            Some(OpCode::InvalidSession) => {
                tracing::warn!(shard_id=?self.shard.id(), "Session was invalidated");

                state.info.invalidate_session();

                state.info_changed = true;
            }

            _ => {}
        }

        false
    }
}
