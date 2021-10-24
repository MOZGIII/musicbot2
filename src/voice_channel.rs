use twilight_gateway::{shard::CommandError, Shard};
use twilight_model::{
    gateway::payload::outgoing::UpdateVoiceState,
    id::{ChannelId, GuildId},
};

pub async fn join(
    shard: &Shard,
    guild_id: impl Into<GuildId>,
    channel_id: impl Into<ChannelId>,
) -> Result<(), CommandError> {
    shard
        .command(&UpdateVoiceState::new(
            guild_id,
            Some(channel_id.into()),
            false,
            false,
        ))
        .await
}

pub async fn leave(shard: &Shard, guild_id: impl Into<GuildId>) -> Result<(), CommandError> {
    shard
        .command(&UpdateVoiceState::new(guild_id, None, false, false))
        .await
}
