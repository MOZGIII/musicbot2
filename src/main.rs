use anyhow::Context;
use futures::StreamExt;
use reqwest::Client as ReqwestClient;
use std::{convert::TryInto, env, future::Future, net::ToSocketAddrs, sync::Arc};
use thiserror::Error;
use tracing::{debug, info};
use twilight_gateway::{Event, Shard};
use twilight_http::Client as HttpClient;
use twilight_lavalink::{
    http::{LoadedTracks, Track},
    model::{Destroy, Pause, Play, Seek, Volume},
    Lavalink,
};
use twilight_model::{
    channel::Message,
    gateway::payload::MessageCreate,
    id::{ChannelId, GuildId, UserId},
};
use twilight_standby::Standby;

mod voice_channel;

#[derive(Clone, Debug)]
struct State {
    http: HttpClient,
    lavalink: Lavalink,
    reqwest: ReqwestClient,
    shard: Shard,
    standby: Standby,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize the tracing subscriber.
    tracing_subscriber::fmt::init();

    let state = {
        let token =
            env::var("DISCORD_TOKEN").with_context(|| "unable to obtain DISCORD_TOKEN env var")?;
        let lavalink_host = env::var("LAVALINK_HOST")
            .with_context(|| "unable to obtain LAVALINK_HOST env var")?
            .to_socket_addrs()
            .with_context(|| "unable to parse lavalink host")?
            .next()
            .with_context(|| "unable to resolve lavalink host")?;
        let lavalink_auth = env::var("LAVALINK_AUTHORIZATION")
            .with_context(|| "unable to obtain LAVALINK_AUTHORIZATION env var")?;
        let shard_count = 1u64;

        let http = HttpClient::new(&token);
        let user_id = http.current_user().await?.id;

        let lavalink = Lavalink::new(user_id, shard_count);
        lavalink.add(lavalink_host, lavalink_auth).await?;

        let mut shard = Shard::new(token);
        shard.start().await?;

        State {
            http,
            lavalink,
            reqwest: ReqwestClient::new(),
            shard,
            standby: Standby::new(),
        }
    };

    let state = Arc::new(state);

    let mut events = state.shard.events();

    info!(message = "processing events");

    while let Some(event) = events.next().await {
        state.standby.process(&event);
        state.lavalink.process(&event).await?;

        if let Event::MessageCreate(msg) = event {
            state.process_message(msg).await;
        }
    }

    Ok(())
}

fn spawn<F>(fut: F)
where
    F: Future<Output = Result<(), anyhow::Error>> + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(why) = fut.await {
            debug!("handler error: {:?}", why);
        }
    });
}

impl State {
    async fn process_message(self: &Arc<Self>, msg: Box<MessageCreate>) {
        let msg: Message = msg.0;

        let guild_id = match msg.guild_id {
            Some(val) => val,
            None => {
                debug!(message = "skipping non-guild message", ?msg);
                return;
            }
        };

        let content = msg.content.to_owned();
        let args: Vec<String> = content.split(' ').map(ToOwned::to_owned).collect();
        let mut args = args.into_iter();

        let command = match args.next() {
            Some(val) => val,
            None => {
                debug!(message = "skipping message without a command", ?msg);
                return;
            }
        };

        let state = Arc::clone(&self);
        match command.as_ref() {
            "!play" => spawn(async move {
                let identifier = match args.next() {
                    Some(val) => val,
                    None => {
                        state
                            .respond_to(msg.channel_id, "Pass track as an argument")
                            .await?;
                        return Ok(());
                    }
                };
                let channel_id = match state.user_voice_channel(guild_id, msg.author.id).await? {
                    Some(val) => val,
                    None => {
                        state
                            .respond_to(msg.channel_id, "You need to join a voice channel first")
                            .await?;
                        return Ok(());
                    }
                };
                match state.action_play(guild_id, channel_id, identifier).await {
                    Ok(track) => {
                        state
                            .respond_to(
                                msg.channel_id,
                                format!(
                                    "Playing **{:?}** by **{:?}**",
                                    track.info.title, track.info.author
                                ),
                            )
                            .await?;
                        Ok(())
                    }
                    Err(err) if err.is::<NoTracksFound>() => {
                        state.respond_to(msg.channel_id, "No tracks found").await?;
                        Ok(())
                    }
                    Err(err) => Err(err)?,
                }
            }),
            "!stop" => spawn(async move { state.action_stop(guild_id).await }),
            "!volume" => spawn(async move {
                let value = match args.next() {
                    Some(val) => val,
                    None => {
                        state
                            .respond_to(msg.channel_id, "Pass volume value as an argument")
                            .await?;
                        return Ok(());
                    }
                };
                let value = match value.parse() {
                    Ok(value) => value,
                    Err(err) => {
                        state
                            .respond_to(msg.channel_id, format!("Volume value is invalid: {}", err))
                            .await?;
                        return Ok(());
                    }
                };
                match state.action_volume(guild_id, value).await {
                    Ok(val) => {
                        state
                            .respond_to(msg.channel_id, format!("Volume was set to {}", val))
                            .await?;
                        Ok(())
                    }
                    Err(err) if err.is::<VolumeValueOutOfBounds>() => {
                        state
                            .respond_to(msg.channel_id, format!("Invalid volume value: {}", err))
                            .await?;
                        Ok(())
                    }
                    Err(err) => Err(err)?,
                }
            }),
            "!seek" => spawn(async move {
                let value = match args.next() {
                    Some(val) => val,
                    None => {
                        state
                            .respond_to(
                                msg.channel_id,
                                "Pass seek position in milliseconds as an argument",
                            )
                            .await?;
                        return Ok(());
                    }
                };
                let value = match value.parse() {
                    Ok(value) => value,
                    Err(err) => {
                        state
                            .respond_to(msg.channel_id, format!("Position is invalid: {}", err))
                            .await?;
                        return Ok(());
                    }
                };
                match state.action_seek(guild_id, value).await {
                    Ok(val) => {
                        state
                            .respond_to(msg.channel_id, format!("Position was set to {}ms", val))
                            .await?;
                        Ok(())
                    }
                    Err(err) => Err(err)?,
                }
            }),
            "!pause" => spawn(async move {
                match state.action_pause_toggle(guild_id).await {
                    Ok(val) => {
                        state
                            .respond_to(msg.channel_id, if val { "Paused" } else { "Unpaused" })
                            .await?;
                        Ok(())
                    }
                    Err(err) => Err(err)?,
                }
            }),
            _ => {}
        }
    }

    async fn user_voice_channel(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<Option<ChannelId>, anyhow::Error> {
        let guild = self
            .http
            .guild(guild_id)
            .await?
            .with_context(|| "no guild")?;
        Ok(guild
            .voice_states
            .get(&user_id)
            .and_then(|voice_state| voice_state.channel_id))
    }

    async fn respond_to(
        &self,
        to: ChannelId,
        with: impl Into<String>,
    ) -> Result<(), anyhow::Error> {
        self.http.create_message(to).content(with)?.await?;
        Ok(())
    }

    async fn action_play(
        &self,
        guild_id: GuildId,
        channel_id: ChannelId,
        identifier: impl AsRef<str>,
    ) -> Result<Track, anyhow::Error> {
        // Join channel.
        voice_channel::join(&self.shard, guild_id, channel_id).await?;

        // Select player.
        let player = self.lavalink.player(guild_id).await?;
        let node_config = player.node().config();

        // Load tracks.
        let req = twilight_lavalink::http::load_track(
            node_config.address,
            identifier,
            &node_config.authorization,
        )?
        .try_into()?;
        let res = self.reqwest.execute(req).await?;
        let loaded = res.json::<LoadedTracks>().await?;

        // Determine the track.
        let mut tracks = loaded.tracks.into_iter();
        let track = tracks.next().ok_or_else(|| NoTracksFound)?;

        // Issue play command.
        player.send(Play::from((guild_id, &track.track)))?;

        // Report success.
        Ok(track)
    }

    async fn action_stop(&self, guild_id: GuildId) -> Result<(), anyhow::Error> {
        // Issue stop command.
        let player = self.lavalink.player(guild_id).await?;
        player.send(Destroy::from(guild_id))?;

        // Leave the voice channel.
        voice_channel::leave(&self.shard, guild_id).await?;

        // Report success.
        Ok(())
    }

    async fn action_volume(&self, guild_id: GuildId, volume: i64) -> Result<i64, anyhow::Error> {
        // Validate input bounds.
        if 0 < volume || volume > 1000 {
            return Err(VolumeValueOutOfBounds(volume).into());
        }

        // Issue volume command.
        let player = self.lavalink.player(guild_id).await?;
        player.send(Volume::from((guild_id, volume)))?;

        // Report success.
        Ok(volume)
    }

    async fn action_seek(
        &self,
        guild_id: GuildId,
        position_in_millis: i64,
    ) -> Result<i64, anyhow::Error> {
        // Issue seek command.
        let player = self.lavalink.player(guild_id).await?;
        player.send(Seek::from((guild_id, position_in_millis)))?;

        // Report success.
        Ok(position_in_millis)
    }

    async fn action_pause_toggle(&self, guild_id: GuildId) -> Result<bool, anyhow::Error> {
        // Prepare and issue pause toggle command.
        let player = self.lavalink.player(guild_id).await?;
        let was_paused = player.paused();
        let should_be_paused = !was_paused;
        player.send(Pause::from((guild_id, should_be_paused)))?;
        Ok(should_be_paused)
    }
}

#[derive(Debug, Error)]
#[error("no tracks found")]
struct NoTracksFound;

#[derive(Debug, Error)]
#[error("volume value is out of bounds: {0}")]
struct VolumeValueOutOfBounds(i64);
