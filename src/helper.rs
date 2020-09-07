use crate::State;
use tracing::debug;
use twilight_model::id::{ChannelId, GuildId, UserId};

pub async fn respond_to(
    state: &State,
    to: ChannelId,
    with: impl Into<String>,
) -> Result<(), anyhow::Error> {
    state.http.create_message(to).content(with)?.await?;
    Ok(())
}

pub async fn user_voice_channel(
    state: &State,
    guild_id: GuildId,
    user_id: UserId,
) -> Result<Option<ChannelId>, anyhow::Error> {
    let user_voice_state = state.cache.voice_state(user_id, guild_id);
    let user_voice_state = match user_voice_state {
        Some(val) => val,
        None => {
            debug!(message = "unable to find user voice state in cache");
            return Ok(None);
        }
    };

    debug!(message = "got user voice state", ?user_voice_state);

    let channel_id = user_voice_state.channel_id;
    let channel_id = match channel_id {
        Some(val) => val,
        None => {
            debug!(message = "unable to find channel id in user voice state");
            return Ok(None);
        }
    };

    debug!(
        message = "got channel id from user voice state",
        ?channel_id
    );

    Ok(Some(channel_id))
}
