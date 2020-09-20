use twilight_model::id::{ChannelId, GuildId};

use crate::player;

#[derive(Debug, Default)]
pub struct Store {
    map: dashmap::DashMap<GuildId, PerGuildData>,
}

impl Store {
    pub fn associate_text_channel(&self, guild_id: GuildId, channel_id: ChannelId) {
        self.map
            .entry(guild_id)
            .and_modify(|data| {
                data.associated_text_channel = Some(channel_id);
            })
            .or_insert_with(|| PerGuildData {
                associated_text_channel: Some(channel_id),
                ..Default::default()
            });
    }

    pub fn get_associated_text_channel(&self, guild_id: GuildId) -> Option<ChannelId> {
        let data = self.map.get(&guild_id)?;
        data.associated_text_channel.clone()
    }

    pub fn with_track_manger<F, V>(&self, guild_id: GuildId, f: F) -> V
    where
        F: FnOnce(&mut player::TrackManager) -> V,
    {
        let mut data = self.map.entry(guild_id).or_default();
        f(&mut data.track_manager)
    }
}

#[derive(Debug, Default)]
struct PerGuildData {
    pub associated_text_channel: Option<ChannelId>,
    pub track_manager: player::TrackManager,
}
