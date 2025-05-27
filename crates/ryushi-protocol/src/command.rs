use serde::{Deserialize, Serialize};
use twilight_model::gateway::payload::outgoing::request_guild_members::RequestGuildMembersInfo;
use twilight_model::gateway::payload::outgoing::update_presence::UpdatePresencePayload;
use twilight_model::gateway::payload::outgoing::update_voice_state::UpdateVoiceStateInfo;
use twilight_model::gateway::payload::outgoing::{
    RequestGuildMembers, UpdatePresence, UpdateVoiceState
};
use twilight_model::gateway::{CloseFrame, OpCode};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CloseStyle {
    /// Normal close code indicating the shard will not be reconnecting soon or
    /// the Shard shouldn't be resumed.
    Normal,

    /// Close code indicating the shard will be reconnecting soon.
    Resume
}

impl From<CloseStyle> for CloseFrame<'static> {
    fn from(val: CloseStyle) -> Self {
        match val {
            CloseStyle::Normal => CloseFrame::NORMAL,
            CloseStyle::Resume => CloseFrame::RESUME
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(tag = "op", content = "d")]
pub enum GatewayCommand {
    /// Request members of a Guild.
    RequestGuildMembers(RequestGuildMembersInfo),

    /// Update the voice state of the shard in a Guild.
    UpdateVoiceState(UpdateVoiceStateInfo),

    /// Update the presence of the shard.
    UpdatePresence(UpdatePresencePayload)
}

impl GatewayCommand {
    pub fn encode(self) -> Result<String, serde_json::Error> {
        match self {
            Self::RequestGuildMembers(d) => {
                serde_json::to_string(&RequestGuildMembers { op: OpCode::RequestGuildMembers, d })
            }
            Self::UpdateVoiceState(d) => {
                serde_json::to_string(&UpdateVoiceState { op: OpCode::VoiceStateUpdate, d })
            }
            Self::UpdatePresence(d) => {
                serde_json::to_string(&UpdatePresence { op: OpCode::PresenceUpdate, d })
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(tag = "t", content = "d", rename_all = "snake_case")]
pub enum ShardCommand {
    /// Send a command to the gateway.
    Gateway(GatewayCommand),

    /// Reconnect the shard.
    Reconnect { close_style: CloseStyle }
}

impl ShardCommand {
    /// Create a new gateway command.
    #[must_use]
    pub const fn update_voice_state(d: UpdateVoiceStateInfo) -> Self {
        Self::Gateway(GatewayCommand::UpdateVoiceState(d))
    }

    /// Create a new reconnect command.
    #[must_use]
    pub const fn reconnect(close_style: CloseStyle) -> Self {
        Self::Reconnect { close_style }
    }
}
