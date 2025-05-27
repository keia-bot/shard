use std::time::Duration;

use futures_util::Stream;
use kameo::reply::ReplyError;
use ryushi_protocol::command::ShardCommand;
use ryushi_protocol::info::ShardInfo;
use twilight_gateway::ShardId;

use crate::sidecar::Dispatch;

/// Represents the data layer between the shard and the application.
pub trait DataLayer: Send + Sync + 'static {
    type Error: ReplyError;
    type Commands: Stream<Item = Result<ShardCommand, Self::Error>> + Unpin + Send;

    /// Saves the shard information to the storage.
    fn shard_save(
        &self, shard_id: ShardId, info: &ShardInfo
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Loads the shard information from the storage.
    fn shard_load(
        &self, shard_id: ShardId
    ) -> impl Future<Output = Result<Option<ShardInfo>, Self::Error>> + Send;

    /// Subscribe to the commands for a given shard.
    fn shard_subscribe(
        &self, shard_id: ShardId
    ) -> impl Future<Output = Result<Self::Commands, Self::Error>> + Send;

    /// Observe an event for a shard.
    fn observe_event(
        &self, shard_id: ShardId, dispatch: Dispatch
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Oberserve the RTT for a heartbeat.
    fn observe_heartbeat_rtt(
        &self, shard_id: ShardId, latency: Duration
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
