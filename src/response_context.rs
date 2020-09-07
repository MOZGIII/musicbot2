use crate::State;
use std::sync::Arc;
use twilight_http::request::prelude::CreateMessage;
use twilight_model::{channel::Message, id::ChannelId};

#[derive(Debug, Clone)]
pub struct ResponseContext {
    state: Arc<State>,
    channel_id: ChannelId,
}

impl ResponseContext {
    pub fn new(state: Arc<State>, to: &Message) -> Self {
        Self {
            state,
            channel_id: to.channel_id,
        }
    }

    pub async fn with<F>(&self, f: F) -> Result<Message, anyhow::Error>
    where
        F: for<'msg> FnOnce(CreateMessage<'msg>) -> Result<CreateMessage<'msg>, anyhow::Error>,
    {
        let msg = self.state.http.create_message(self.channel_id);
        let msg = f(msg)?;
        let val = msg.await?;
        Ok(val)
    }

    pub async fn with_content(&self, content: impl Into<String>) -> Result<Message, anyhow::Error> {
        self.with(|msg| Ok(msg.content(content)?)).await
    }
}
