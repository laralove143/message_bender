#![warn(clippy::cargo, clippy::nursery, clippy::pedantic, clippy::restriction)]
#![allow(
    clippy::blanket_clippy_restriction_lints,
    clippy::missing_docs_in_private_items,
    clippy::implicit_return,
    clippy::missing_errors_doc,
    clippy::pattern_type_mismatch
)]

mod interaction;

use std::sync::Arc;

use anyhow::IntoResult;
use futures_util::StreamExt;
use twilight_cache_inmemory::{InMemoryCache, ResourceType};
use twilight_error::ErrorHandler;
use twilight_gateway::{Cluster, EventTypeFlags};
use twilight_http::Client;
use twilight_model::{
    application::interaction::Interaction,
    gateway::{
        event::Event, payload::outgoing::request_guild_members::RequestGuildMembersBuilder, Intents,
    },
    guild::Guild,
    id::{
        marker::{ApplicationMarker, GuildMarker, UserMarker},
        Id,
    },
};
use twilight_webhook::cache::WebhooksCache;

pub struct Context {
    http: Client,
    cache: InMemoryCache,
    webhooks_cache: WebhooksCache,
    application_id: Id<ApplicationMarker>,
    user_id: Id<UserMarker>,
    error_handler: ErrorHandler,
}

impl Context {
    async fn handle_event(self: Arc<Self>, event: Event) {
        if let Err(err) = self._handle_event(event).await {
            self.error_handler.handle(&self.http, err).await;
        }
    }

    async fn _handle_event(&self, event: Event) -> Result<(), anyhow::Error> {
        if let Event::InteractionCreate(mut interaction) = event {
            self.interaction_handler(&mut interaction.0)?
                .handle(interaction.0)
                .await?;
        }
        Ok(())
    }

    async fn request_members(&self, cluster: Arc<Cluster>, shard_id: u64, guild: &Guild) {
        if let Err(err) = cluster
            .command(
                shard_id,
                &RequestGuildMembersBuilder::new(guild.id).query("", None),
            )
            .await
        {
            self.error_handler.handle(&self.http, err).await;
        }
    }

    pub fn interaction_handler(
        &self,
        interaction: &mut Interaction,
    ) -> Result<interaction::Handler<'_>, anyhow::Error> {
        interaction::Handler::new(self, interaction)
    }

    pub async fn create_commands(
        &self,
        test_guild_id: Option<Id<GuildMarker>>,
    ) -> Result<(), anyhow::Error> {
        interaction::create_commands(&self.http, self.application_id, test_guild_id).await?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenvy::dotenv()?;
    
    let intents = Intents::MESSAGE_CONTENT
        | Intents::GUILD_MESSAGES
        | Intents::GUILD_MESSAGE_REACTIONS
        | Intents::GUILDS;
    let event_types = EventTypeFlags::INTERACTION_CREATE
        | EventTypeFlags::GUILD_MESSAGES
        | EventTypeFlags::GUILD_MESSAGE_REACTIONS
        | EventTypeFlags::GUILDS
        | EventTypeFlags::THREAD_CREATE
        | EventTypeFlags::THREAD_UPDATE
        | EventTypeFlags::THREAD_DELETE
        | EventTypeFlags::THREAD_LIST_SYNC
        | EventTypeFlags::THREAD_MEMBER_UPDATE
        | EventTypeFlags::THREAD_MEMBERS_UPDATE
        | EventTypeFlags::GUILD_MEMBERS
        | EventTypeFlags::MEMBER_CHUNK;
    let resource_types = ResourceType::MESSAGE
        | ResourceType::REACTION
        | ResourceType::GUILD
        | ResourceType::CHANNEL
        | ResourceType::MEMBER
        | ResourceType::USER
        | ResourceType::ROLE;

    let test_guild_id: Option<Id<GuildMarker>> =
        option_env!("TEST_GUILD_ID").and_then(|id| id.parse().ok());

    let token = test_guild_id
        .is_some()
        .then(|| option_env!("TEST_BOT_TOKEN"))
        .unwrap_or(option_env!("EDIT_BOT_TOKEN"))
        .ok()?
        .to_owned();

    let (cluster, mut events) = Cluster::builder(token.clone(), intents)
        .event_types(event_types)
        .build()
        .await?;
    let cluster_arc = Arc::new(cluster);

    let cluster_spawn = Arc::clone(&cluster_arc);
    tokio::spawn(async move { cluster_spawn.up().await });

    let http = Client::new(token);

    let application = http
        .current_user_application()
        .exec()
        .await?
        .model()
        .await?;
    let application_id = application.id;
    let user_id = http.current_user().exec().await?.model().await?.id;

    let cache = InMemoryCache::builder()
        .resource_types(resource_types)
        .message_cache_size(24)
        .build();

    let webhooks_cache = WebhooksCache::new();

    let mut error_handler = ErrorHandler::new();
    error_handler
        .file("edit_any_message_bot_errors.txt".into())
        .channel(
            http.create_private_channel(application.owner.ok()?.id)
                .exec()
                .await?
                .model()
                .await?
                .id,
        );

    let ctx = Arc::new(Context {
        http,
        cache,
        webhooks_cache,
        application_id,
        user_id,
        error_handler,
    });

    ctx.create_commands(test_guild_id).await?;

    while let Some((shard_id, event)) = events.next().await {
        ctx.cache.update(&event);
        if let Event::GuildCreate(guild) = &event {
            ctx.request_members(Arc::clone(&cluster_arc), shard_id, guild)
                .await;
        }
        tokio::spawn(Arc::clone(&ctx).handle_event(event));
    }

    Ok(())
}
