#![warn(clippy::cargo, clippy::nursery, clippy::pedantic, clippy::restriction)]
#![allow(
    clippy::blanket_clippy_restriction_lints,
    clippy::missing_docs_in_private_items,
    clippy::implicit_return,
    clippy::multiple_inherent_impl,
    clippy::missing_errors_doc
)]

mod interaction;

use std::{env, ops::Deref, sync::Arc};

use anyhow::Result;
use futures_util::StreamExt;
use twilight_cache_inmemory::{InMemoryCache, ResourceType};
use twilight_gateway::{Cluster, EventTypeFlags};
use twilight_http::Client;
use twilight_model::{
    gateway::{event::Event, Intents},
    id::{
        marker::{ApplicationMarker, GuildMarker},
        Id,
    },
};

pub struct ContextInner {
    http: Client,
    cache: InMemoryCache,
    application_id: Id<ApplicationMarker>,
}

pub struct Context(Arc<ContextInner>);

impl Clone for Context {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl Deref for Context {
    type Target = Arc<ContextInner>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Context {
    #[allow(clippy::print_stderr, clippy::wildcard_enum_match_arm)]
    async fn handle_event(self, event: Event) {
        if let Err(err) = match event {
            Event::InteractionCreate(interaction) => self.handle_interaction(interaction.0).await,
            _ => Ok(()),
        } {
            eprintln!("{err}");
            if let Err(e) = self.inform_error().await {
                eprintln!("error when informing owner: {e}");
            }
        }
    }

    async fn inform_error(&self) -> Result<()> {
        self.http
            .create_message(
                self.http
                    .create_private_channel(
                        self.http
                            .current_user_application()
                            .exec()
                            .await?
                            .model()
                            .await?
                            .owner
                            .id,
                    )
                    .exec()
                    .await?
                    .model()
                    .await?
                    .id,
            )
            .content("an error occurred :(")?
            .exec()
            .await?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // todo: add tracing
    let intents = Intents::MESSAGE_CONTENT | Intents::GUILD_MESSAGES;
    let event_types = EventTypeFlags::INTERACTION_CREATE | EventTypeFlags::GUILD_MESSAGES;
    let resource_types = ResourceType::MESSAGE;

    let test_guild_id: Option<Id<GuildMarker>> = env::var("TEST_GUILD_ID")
        .ok()
        .and_then(|id| id.parse().ok())
        .map(Id::new);

    let token = env::var(if test_guild_id.is_some() {
        "TEST_BOT_TOKEN"
    } else {
        "EDIT_BOT_TOKEN"
    })?;

    let (cluster, mut events) = Cluster::builder(token.clone(), intents)
        .event_types(event_types)
        .build()
        .await?;
    let cluster_spawn = Arc::new(cluster);
    tokio::spawn(async move { cluster_spawn.up().await });

    let http = Client::new(token);

    let application_id = http
        .current_user_application()
        .exec()
        .await?
        .model()
        .await?
        .id;

    let cache = InMemoryCache::builder()
        .resource_types(resource_types)
        .message_cache_size(25)
        .build();

    let ctx = Context(Arc::new(ContextInner {
        http,
        cache,
        application_id,
    }));

    ctx.create_commands(test_guild_id).await?;

    while let Some((_, event)) = events.next().await {
        ctx.cache.update(&event);
        tokio::spawn(ctx.clone().handle_event(event));
    }

    Ok(())
}
