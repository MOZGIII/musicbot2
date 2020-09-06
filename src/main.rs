use anyhow::Context;
use futures::StreamExt;
use reqwest::Client as ReqwestClient;
use std::{convert::TryInto, env, future::Future, net::ToSocketAddrs, sync::Arc};
use tracing::{debug, info};
use twilight_gateway::{Event, Shard};
use twilight_http::Client as HttpClient;
use twilight_lavalink::{
    http::LoadedTracks,
    model::{Destroy, Pause, Play, Seek, Stop, Volume},
    Lavalink,
};
use twilight_model::{channel::Message, gateway::payload::MessageCreate, id::ChannelId};
use twilight_standby::Standby;

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
            if msg.guild_id.is_none() || !msg.content.starts_with('!') {
                continue;
            }

            let state = Arc::clone(&state);
            match msg.content.splitn(2, ' ').next() {
                Some("!join") => spawn(async move { state.join(msg.0).await }),
                Some("!leave") => spawn(async move { state.leave(msg.0).await }),
                Some("!pause") => spawn(async move { state.pause(msg.0).await }),
                Some("!play") => spawn(async move { state.play(msg.0).await }),
                Some("!seek") => spawn(async move { state.seek(msg.0).await }),
                Some("!stop") => spawn(async move { state.stop(msg.0).await }),
                Some("!volume") => spawn(async move { state.volume(msg.0).await }),
                _ => continue,
            }
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
    async fn join(&self, msg: Message) -> Result<(), anyhow::Error> {
        debug!(
            message = "handling command",
            command = "join",
            channel = %msg.channel_id,
            author = %msg.author.name,
        );

        self.http
            .create_message(msg.channel_id)
            .content("What's the channel ID you want me to join?")?
            .await?;

        let author_id = msg.author.id;
        let msg = self
            .standby
            .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
                new_msg.author.id == author_id
            })
            .await?;
        let channel_id = msg.content.parse::<u64>()?;

        self.shard
            .command(&serde_json::json!({
                "op": 4,
                "d": {
                    "channel_id": channel_id,
                    "guild_id": msg.guild_id,
                    "self_mute": false,
                    "self_deaf": false,
                }
            }))
            .await?;

        self.http
            .create_message(msg.channel_id)
            .content(format!("Joined <#{}>!", channel_id))?
            .await?;

        Ok(())
    }

    async fn leave(&self, msg: Message) -> Result<(), anyhow::Error> {
        debug!(
            message = "handling command",
            command = "leave",
            channel = %msg.channel_id,
            author = %msg.author.name,
        );

        let guild_id = msg.guild_id.unwrap();
        let player = self.lavalink.player(guild_id).await.unwrap();
        player.send(Destroy::from(guild_id))?;
        self.shard
            .command(&serde_json::json!({
                "op": 4,
                "d": {
                    "channel_id": None::<ChannelId>,
                    "guild_id": msg.guild_id,
                    "self_mute": false,
                    "self_deaf": false,
                }
            }))
            .await?;

        self.http
            .create_message(msg.channel_id)
            .content("Left the channel")?
            .await?;

        Ok(())
    }

    async fn play(&self, msg: Message) -> Result<(), anyhow::Error> {
        debug!(
            message = "handling command",
            command = "play",
            channel = %msg.channel_id,
            author = %msg.author.name,
        );

        self.http
            .create_message(msg.channel_id)
            .content("What's the URL of the audio to play?")?
            .await?;

        let author_id = msg.author.id;
        let msg = self
            .standby
            .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
                new_msg.author.id == author_id
            })
            .await?;
        let guild_id = msg.guild_id.unwrap();

        let player = self.lavalink.player(guild_id).await.unwrap();
        let req = twilight_lavalink::http::load_track(
            player.node().config().address,
            &msg.content,
            &player.node().config().authorization,
        )?
        .try_into()?;
        let res = self.reqwest.execute(req).await?;
        let loaded = res.json::<LoadedTracks>().await?;

        if let Some(track) = loaded.tracks.first() {
            player.send(Play::from((guild_id, &track.track)))?;

            let content = format!(
                "Playing **{:?}** by **{:?}**",
                track.info.title, track.info.author
            );
            self.http
                .create_message(msg.channel_id)
                .content(content)?
                .await?;
        } else {
            self.http
                .create_message(msg.channel_id)
                .content("Didn't find any results")?
                .await?;
        }

        Ok(())
    }

    async fn pause(&self, msg: Message) -> Result<(), anyhow::Error> {
        debug!(
            message = "handling command",
            command = "pause",
            channel = %msg.channel_id,
            author = %msg.author.name,
        );

        let guild_id = msg.guild_id.unwrap();
        let player = self.lavalink.player(guild_id).await.unwrap();
        let paused = player.paused();
        player.send(Pause::from((guild_id, !paused)))?;

        let action = if paused { "Unpaused " } else { "Paused" };

        self.http
            .create_message(msg.channel_id)
            .content(format!("{} the track", action))?
            .await?;

        Ok(())
    }

    async fn seek(&self, msg: Message) -> Result<(), anyhow::Error> {
        debug!(
            message = "handling command",
            command = "seek",
            channel = %msg.channel_id,
            author = %msg.author.name,
        );

        self.http
            .create_message(msg.channel_id)
            .content("Where in the track do you want to seek to (in seconds)?")?
            .await?;

        let author_id = msg.author.id;
        let msg = self
            .standby
            .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
                new_msg.author.id == author_id
            })
            .await?;
        let guild_id = msg.guild_id.unwrap();
        let position = msg.content.parse::<i64>()?;

        let player = self.lavalink.player(guild_id).await.unwrap();
        player.send(Seek::from((guild_id, position * 1000)))?;

        self.http
            .create_message(msg.channel_id)
            .content(format!("Seeked to {}s", position))?
            .await?;

        Ok(())
    }

    async fn stop(&self, msg: Message) -> Result<(), anyhow::Error> {
        debug!(
            message = "handling command",
            command = "stop",
            channel = %msg.channel_id,
            author = %msg.author.name,
        );

        let guild_id = msg.guild_id.unwrap();
        let player = self.lavalink.player(guild_id).await.unwrap();
        player.send(Stop::from(guild_id))?;

        self.http
            .create_message(msg.channel_id)
            .content("Stopped the track")?
            .await?;

        Ok(())
    }

    async fn volume(&self, msg: Message) -> Result<(), anyhow::Error> {
        debug!(
            message = "handling command",
            command = "volume",
            channel = %msg.channel_id,
            author = %msg.author.name,
        );

        self.http
            .create_message(msg.channel_id)
            .content("What's the volume you want to set (0-1000, 100 being the default)?")?
            .await?;

        let author_id = msg.author.id;
        let msg = self
            .standby
            .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
                new_msg.author.id == author_id
            })
            .await?;
        let guild_id = msg.guild_id.unwrap();
        let volume = msg.content.parse::<i64>()?;

        if volume > 1000 || volume < 0 {
            self.http
                .create_message(msg.channel_id)
                .content("That's more than 1000")?
                .await?;

            return Ok(());
        }

        let player = self.lavalink.player(guild_id).await.unwrap();
        player.send(Volume::from((guild_id, volume)))?;

        self.http
            .create_message(msg.channel_id)
            .content(format!("Set the volume to {}", volume))?
            .await?;

        Ok(())
    }
}
