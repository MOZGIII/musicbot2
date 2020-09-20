use reqwest::Client as ReqwestClient;
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::Shard;
use twilight_http::Client as HttpClient;
use twilight_lavalink::Lavalink;
use twilight_standby::Standby;

use crate::per_guild_data::Store;

#[derive(Debug)]
pub struct State {
    pub http: HttpClient,
    pub lavalink: Lavalink,
    pub reqwest: ReqwestClient,
    pub shard: Shard,
    pub standby: Standby,
    pub cache: InMemoryCache,
    pub command_prefix: String,
    pub per_guild_data: Store,
}
