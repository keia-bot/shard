use std::time::Duration;

use chrono::serde::{ts_milliseconds, ts_milliseconds_option};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::{DurationMilliSeconds, serde_as};
use twilight_gateway::{Session, ShardId, ShardState};

#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
#[serde(remote = "ShardState", rename_all = "snake_case", tag = "name")]
enum ShardStateDef {
    Active,
    Disconnected { reconnect_attempts: u8 },
    FatallyClosed,
    Identifying,
    Resuming
}

#[serde_as]
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Heartbeat {
    #[serde(with = "ts_milliseconds")]
    pub at: DateTime<Utc>,
    #[serde_as(as = "DurationMilliSeconds")]
    pub rtt: Duration
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ShardInfo {
    /// The ID of the cluster the shard is in.
    pub cluster_id: Option<String>,
    /// The ID of the shard.
    pub shard_id: ShardId,
    /// The session to use to resume the session.
    pub session: Option<Session>,
    /// The time the shard is ready to resume.
    #[serde(with = "ts_milliseconds_option")]
    pub ready_at: Option<DateTime<Utc>>,
    /// The Gateway URL to use to resume the session.
    pub resume_url: Option<String>,
    /// Whether the shard is fully ready, i.e., all initial guilds have been
    /// received.
    pub full_ready: bool,
    /// The current state of the shard.
    #[serde(with = "ShardStateDef")]
    pub state: ShardState,
    /// The last heartbeat information.
    pub heartbeat: Option<Heartbeat>,
    /// The number of guilds the shard is currently in.
    pub guild_count: u16 // use a u16 since shards are limited to 2500 guilds
}

impl ShardInfo {
    #[must_use]
    pub const fn new(cluster_id: Option<String>, id: ShardId) -> Self {
        Self {
            cluster_id,
            shard_id: id,
            session: None,
            resume_url: None,
            state: ShardState::Disconnected { reconnect_attempts: 0 },
            heartbeat: None,
            guild_count: 0,
            ready_at: None,
            full_ready: false
        }
    }

    pub fn invalidate_session(&mut self) {
        self.session = None;
        self.ready_at = None;
        self.resume_url = None;
        self.heartbeat = None;
        self.full_ready = false;
    }
}
