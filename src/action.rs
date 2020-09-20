use crate::{voice_channel, State};
use std::{convert::TryInto, ops::RangeInclusive};
use thiserror::Error;
use twilight_lavalink::{
    http::{LoadedTracks, Track},
    model::{Destroy, Pause, Play, Seek, Volume},
};
use twilight_model::id::{ChannelId, GuildId};

pub async fn play(
    state: &State,
    guild_id: GuildId,
    channel_id: ChannelId,
    identifier: impl AsRef<str>,
) -> Result<Track, anyhow::Error> {
    // Join channel.
    voice_channel::join(&state.shard, guild_id, channel_id).await?;

    // Select player.
    let player = state.lavalink.player(guild_id).await?;
    let node_config = player.node().config();

    // Load tracks.
    let req = twilight_lavalink::http::load_track(
        node_config.address,
        identifier,
        &node_config.authorization,
    )?
    .try_into()?;
    let res = state.reqwest.execute(req).await?;
    let loaded = res.json::<LoadedTracks>().await?;

    // Determine the track.
    let mut tracks = loaded.tracks.into_iter();
    let track = tracks.next().ok_or_else(|| NoTracksFound)?;

    // Issue play command.
    player.send(Play::new(guild_id, &track.track, None, None, false))?;

    // Report success.
    Ok(track)
}

pub async fn enqueue(
    state: &State,
    guild_id: GuildId,
    channel_id: ChannelId,
    identifier: impl AsRef<str>,
) -> Result<Track, anyhow::Error> {
    // Join channel.
    voice_channel::join(&state.shard, guild_id, channel_id).await?;

    // Select player.
    let player = state.lavalink.player(guild_id).await?;
    let node_config = player.node().config();

    // Load tracks.
    let req = twilight_lavalink::http::load_track(
        node_config.address,
        identifier,
        &node_config.authorization,
    )?
    .try_into()?;
    let res = state.reqwest.execute(req).await?;
    let loaded = res.json::<LoadedTracks>().await?;

    // Determine the track.
    let mut tracks = loaded.tracks.into_iter();
    let track = tracks.next().ok_or_else(|| NoTracksFound)?;

    // Enqueue track.
    state
        .per_guild_data
        .with_track_manger(guild_id, |track_manager| {
            track_manager.enqueue(std::iter::once(track.clone()));
        });

    // Report success.
    Ok(track)
}

pub async fn play_from_queue(
    state: &State,
    guild_id: GuildId,
) -> Result<Option<Track>, anyhow::Error> {
    // Get the track from queue.
    let track = state
        .per_guild_data
        .with_track_manger(guild_id, |track_manager| track_manager.next_track());

    let track = match track {
        Some(val) => val,
        // No track is in queue.
        None => return Ok(None),
    };

    // Select player.
    let player = state.lavalink.player(guild_id).await?;

    // Issue play command.
    player.send(Play::new(guild_id, &track.track, None, None, false))?;

    // Report success.
    Ok(Some(track))
}

pub async fn stop(state: &State, guild_id: GuildId) -> Result<(), anyhow::Error> {
    // Issue stop command.
    let player = state.lavalink.player(guild_id).await?;
    player.send(Destroy::from(guild_id))?;

    // Leave the voice channel.
    voice_channel::leave(&state.shard, guild_id).await?;

    // Report success.
    Ok(())
}

const VOLUME_BOUNDS: RangeInclusive<i64> = 0..=1000;

pub async fn volume(state: &State, guild_id: GuildId, volume: i64) -> Result<i64, anyhow::Error> {
    // Validate input bounds.
    if !VOLUME_BOUNDS.contains(&volume) {
        return Err(VolumeValueOutOfBounds {
            value: volume,
            bounds: VOLUME_BOUNDS,
        }
        .into());
    }

    // Issue volume command.
    let player = state.lavalink.player(guild_id).await?;
    player.send(Volume::from((guild_id, volume)))?;

    // Report success.
    Ok(volume)
}

pub async fn seek(
    state: &State,
    guild_id: GuildId,
    position_in_millis: i64,
) -> Result<i64, anyhow::Error> {
    // Issue seek command.
    let player = state.lavalink.player(guild_id).await?;
    player.send(Seek::from((guild_id, position_in_millis)))?;

    // Report success.
    Ok(position_in_millis)
}

pub async fn pause_toggle(state: &State, guild_id: GuildId) -> Result<bool, anyhow::Error> {
    // Prepare and issue pause toggle command.
    let player = state.lavalink.player(guild_id).await?;
    let was_paused = player.paused();
    let should_be_paused = !was_paused;
    player.send(Pause::from((guild_id, should_be_paused)))?;
    Ok(should_be_paused)
}

#[derive(Debug, Error)]
#[error("no tracks found")]
pub struct NoTracksFound;

#[derive(Debug, Error)]
#[error("volume value is out of bounds: {value}, must be in {bounds:?}")]
pub struct VolumeValueOutOfBounds {
    value: i64,
    bounds: RangeInclusive<i64>,
}
