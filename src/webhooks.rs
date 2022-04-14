use std::ops::Deref;

use anyhow::Ok;
use dashmap::{mapref::one::Ref, DashMap};
use twilight_model::{
    channel::Webhook,
    id::{
        marker::{ChannelMarker, WebhookMarker},
        Id,
    },
};

use crate::Context;

pub struct Cache(DashMap<Id<ChannelMarker>, CachedWebhook>);

impl Deref for Cache {
    type Target = DashMap<Id<ChannelMarker>, CachedWebhook>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Cache {
    pub fn new() -> Self {
        Self(DashMap::new())
    }
}

pub struct CachedWebhook {
    id: Id<WebhookMarker>,
    token: String,
}

impl TryFrom<Webhook> for CachedWebhook {
    type Error = anyhow::Error;

    fn try_from(webhook: Webhook) -> Result<Self, Self::Error> {
        Ok(Self {
            id: webhook.id,
            token: webhook.token.ok()?,
        })
    }
}

impl Context {
    async fn webhook(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<Ref<'_, Id<ChannelMarker>, CachedWebhook>, anyhow::Error> {
        if let Some(webhook) = self.webhooks_cache.get(&channel_id) {
            Ok(webhook)
        } else {
            let webhook = if let Some(webhook) = self
                .http
                .channel_webhooks(channel_id)
                .exec()
                .await?
                .models()
                .await?
                .into_iter()
                .find(|webhook| webhook.application_id == Some(self.application_id))
            {
                webhook
            } else {
                self.http
                    .create_webhook(channel_id, "any message editor")
                    .exec()
                    .await?
                    .model()
                    .await?
            }
            .try_into()?;
            self.webhooks_cache.insert(channel_id, webhook);
            Ok(self.webhooks_cache.get(&channel_id).ok()?)
        }
    }

    pub async fn webhooks_cache_update(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<(), anyhow::Error> {
        if self.webhooks_cache.contains_key(&channel_id)
            && !self
                .http
                .channel_webhooks(channel_id)
                .exec()
                .await?
                .models()
                .await?
                .iter()
                .any(|webhook| webhook.application_id == Some(self.application_id))
        {
            self.webhooks_cache.remove(&channel_id);
        }

        Ok(())
    }
}
