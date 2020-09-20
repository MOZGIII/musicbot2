use twilight_lavalink::http::Track;

#[derive(Debug, Default)]
pub struct TrackManager {
    track_queue: Vec<Track>,
}

impl TrackManager {
    pub fn enqueue<T>(&mut self, tracks: T)
    where
        T: IntoIterator<Item = Track>,
    {
        self.track_queue.extend(tracks)
    }

    pub fn next_track(&mut self) -> Option<Track> {
        self.track_queue.pop()
    }
}
