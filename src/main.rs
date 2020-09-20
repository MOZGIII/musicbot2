use anyhow::Context;
use futures::StreamExt;
use reqwest::Client as ReqwestClient;
use std::{env, future::Future, net::ToSocketAddrs, sync::Arc};
use tracing::{debug, info, trace, warn};
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::{Event, Shard};
use twilight_http::Client as HttpClient;
use twilight_lavalink::Lavalink;
use twilight_model::channel::Message;
use twilight_standby::Standby;

mod action;
mod helper;
mod response_context;
mod state;
mod voice_channel;

use helper::user_voice_channel;
use response_context::ResponseContext;
use state::State;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize the tracing subscriber.
    tracing_subscriber::fmt::init();

    let state = {
        let token =
            env::var("DISCORD_TOKEN").with_context(|| "unable to obtain DISCORD_TOKEN env var")?;
        let command_prefix = env::var("PREFIX").unwrap_or_else(|_| "!".to_owned());
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

        let cache = InMemoryCache::new();

        let mut shard = Shard::new(token);
        shard.start().await?;

        State {
            http,
            lavalink,
            reqwest: ReqwestClient::new(),
            shard,
            standby: Standby::new(),
            cache,
            command_prefix,
        }
    };

    let state = Arc::new(state);

    let mut events = state.shard.events();

    info!(message = "processing events");

    while let Some(event) = events.next().await {
        trace!(message = "start event handling", ?event);
        state.cache.update(&event);
        state.standby.process(&event);
        let event2 = event.clone();
        let state2 = Arc::clone(&state);
        spawn(async move {
            state2.lavalink.process(&event2).await?;
            Ok(())
        });
        process_event(&state, &event);
        trace!(message = "finish event handling", ?event);
    }

    Ok(())
}

fn spawn<F>(fut: F)
where
    F: Future<Output = Result<(), anyhow::Error>> + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(why) = fut.await {
            warn!("handler error: {:?}", why);
        }
    });
}

fn process_event(state: &Arc<State>, event: &Event) {
    let msg = match event {
        Event::MessageCreate(msg) => msg,
        _ => return,
    };

    let msg: &Message = &msg.0;

    let guild_id = match msg.guild_id {
        Some(val) => val,
        None => {
            debug!(message = "skipping non-guild message", ?msg);
            return;
        }
    };

    let command = match msg.content.strip_prefix(&state.command_prefix) {
        Some(val) => val,
        None => {
            debug!(message = "skipping non-command message", ?msg);
            return;
        }
    };

    let args: Vec<String> = command.split(' ').map(ToOwned::to_owned).collect();
    let mut args = args.into_iter();

    let command = match args.next() {
        Some(val) => val,
        None => {
            debug!(message = "skipping message without a command", ?msg);
            return;
        }
    };
    info!(message = "got command", %command, args = ?args.as_slice());

    let response_context = ResponseContext::new(Arc::clone(state), &msg);

    let state = Arc::clone(state);
    match command.as_ref() {
        "play" => {
            let author_id = msg.author.id;
            spawn(async move {
                let identifier = match args.next() {
                    Some(val) => val,
                    None => {
                        response_context
                            .with_content("Pass track as an argument")
                            .await?;
                        return Ok(());
                    }
                };
                let channel_id = match user_voice_channel(&state, guild_id, author_id).await? {
                    Some(val) => val,
                    None => {
                        response_context
                            .with_content("You need to join a voice channel first")
                            .await?;
                        return Ok(());
                    }
                };
                match action::play(&state, guild_id, channel_id, identifier).await {
                    Ok(track) => {
                        response_context
                            .with_content(format!(
                                "Playing **{:?}** by **{:?}**",
                                track.info.title, track.info.author
                            ))
                            .await?;
                        Ok(())
                    }
                    Err(err) if err.is::<action::NoTracksFound>() => {
                        response_context.with_content("No tracks found").await?;
                        Ok(())
                    }
                    Err(err) => Err(err)?,
                }
            })
        }
        "stop" => spawn(async move { action::stop(&state, guild_id).await }),
        "volume" => spawn(async move {
            let value = match args.next() {
                Some(val) => val,
                None => {
                    response_context
                        .with_content("Pass volume value as an argument")
                        .await?;
                    return Ok(());
                }
            };
            let value = match value.parse() {
                Ok(value) => value,
                Err(err) => {
                    response_context
                        .with_content(format!("Volume value is invalid: {}", err))
                        .await?;
                    return Ok(());
                }
            };
            match action::volume(&state, guild_id, value).await {
                Ok(val) => {
                    response_context
                        .with_content(format!("Volume was set to {}", val))
                        .await?;
                    Ok(())
                }
                Err(err) if err.is::<action::VolumeValueOutOfBounds>() => {
                    response_context
                        .with_content(format!("Invalid volume value: {}", err))
                        .await?;
                    Ok(())
                }
                Err(err) => Err(err)?,
            }
        }),
        "seek" => spawn(async move {
            let value = match args.next() {
                Some(val) => val,
                None => {
                    response_context
                        .with_content("Pass seek position in milliseconds as an argument")
                        .await?;
                    return Ok(());
                }
            };
            let value = match value.parse() {
                Ok(value) => value,
                Err(err) => {
                    response_context
                        .with_content(format!("Position is invalid: {}", err))
                        .await?;
                    return Ok(());
                }
            };
            match action::seek(&state, guild_id, value).await {
                Ok(val) => {
                    response_context
                        .with_content(format!("Position was set to {}ms", val))
                        .await?;
                    Ok(())
                }
                Err(err) => Err(err)?,
            }
        }),
        "pause" => spawn(async move {
            match action::pause_toggle(&state, guild_id).await {
                Ok(val) => {
                    response_context
                        .with_content(if val { "Paused" } else { "Unpaused" })
                        .await?;
                    Ok(())
                }
                Err(err) => Err(err)?,
            }
        }),
        "ping" => spawn(async move {
            response_context.with_content("pong").await?;
            Ok(())
        }),
        _ => {}
    }
}
