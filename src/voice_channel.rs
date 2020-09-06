use twilight_gateway::{shard::CommandError, Shard};
use twilight_model::id::{ChannelId, GuildId};

pub async fn join(
    shard: &Shard,
    guild_id: impl Into<GuildId>,
    channel_id: impl Into<ChannelId>,
) -> Result<(), CommandError> {
    shard
        .command(&serde_json::json!({
            "op": 4,
            "d": {
                "channel_id": channel_id.into(),
                "guild_id": guild_id.into(),
                "self_mute": false,
                "self_deaf": false,
            }
        }))
        .await
}

pub async fn leave(shard: &Shard, guild_id: impl Into<GuildId>) -> Result<(), CommandError> {
    shard
        .command(&serde_json::json!({
            "op": 4,
                "d": {
                    "channel_id": None::<ChannelId>,
                    "guild_id": guild_id.into(),
                    "self_mute": false,
                    "self_deaf": false,
                }
        }))
        .await
}
