use std::{fmt::Write, ops::Deref};

use anyhow::Context;
use thiserror::Error;
use twilight_cache_inmemory::{model::CachedMessage, Reference};
use twilight_interactions::command::{CommandModel, CreateCommand};
use twilight_model::{
    application::{
        command::{Command, CommandType},
        component::{text_input::TextInputStyle, ActionRow, Component, TextInput},
        interaction::{modal::ModalSubmitInteraction, ApplicationCommand},
    },
    channel::{
        message::{MessageFlags, MessageType},
        Message,
    },
    guild::Permissions,
    http::interaction::{InteractionResponse, InteractionResponseType},
    id::{marker::MessageMarker, Id},
};
use twilight_util::builder::{command::CommandBuilder, InteractionResponseDataBuilder};
use twilight_webhook::util::{MinimalMember, MinimalWebhook};

use crate::interaction;

#[derive(Error, Debug)]
pub enum Error {
    #[error("this message is weird, it has something i cant recreate like a reaction.. sorry")]
    MessageWeird,
    #[error("this message is too long, someone with nitro sent it but bots dont have nitro sadly")]
    MessageTooLong,
    #[error(
        "i dont know any messages here yet, i can only see messages sent after i joined or got \
         updated.. sorry"
    )]
    NoCachedMessages,
}

#[derive(CreateCommand, CommandModel)]
#[command(name = "edit", desc = "edit any message you select")]
pub struct ChatInput {}

pub struct Handler<'ctx>(&'ctx interaction::Handler<'ctx>);

impl<'ctx> Deref for Handler<'ctx> {
    type Target = interaction::Handler<'ctx>;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'ctx> Handler<'ctx> {
    pub const fn new(interaction_handler: &'ctx interaction::Handler) -> Self {
        Self(interaction_handler)
    }

    pub async fn chat_input_command(&self) -> Result<(), anyhow::Error> {
        self.create_response(&InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(
                InteractionResponseDataBuilder::new()
                    .content(
                        "im now even easier to use, just right click/long press on a message, \
                         select `apps` then `edit`!"
                            .to_owned(),
                    )
                    .flags(MessageFlags::EPHEMERAL)
                    .build(),
            ),
        })
        .await?;
        Ok(())
    }

    pub async fn command(&self, command: ApplicationCommand) -> Result<(), anyhow::Error> {
        if let Err(err) = self._command(command).await {
            self.defer().await?;
            Err(err)
        } else {
            Ok(())
        }
    }

    async fn _command(&self, command: ApplicationCommand) -> Result<(), anyhow::Error> {
        self.check_self_permissions(
            command.channel_id,
            Permissions::MANAGE_MESSAGES | Permissions::MANAGE_WEBHOOKS,
        )?;

        let message = command
            .data
            .resolved
            .context("command data doesn't have resolved data")?
            .messages
            .into_values()
            .next()
            .context("command data doesn't have a message")?;

        if self.cache.message(message.id).is_none() {
            return Err(super::Error::Edit(Error::NoCachedMessages).into());
        };
        if message.content.chars().count() > 2000 {
            return Err(super::Error::Edit(Error::MessageTooLong).into());
        }
        if message_is_weird(&message) {
            return Err(super::Error::Edit(Error::MessageWeird).into());
        }

        self.create_response(&InteractionResponse {
            kind: InteractionResponseType::Modal,
            data: Some(
                InteractionResponseDataBuilder::new()
                    .title("edit message".to_owned())
                    .custom_id("edit_modal".to_owned())
                    .components([Component::ActionRow(ActionRow {
                        components: vec![Component::TextInput(TextInput {
                            custom_id: message.id.to_string(),
                            label: "what to edit the message to".to_owned(),
                            style: TextInputStyle::Paragraph,
                            value: Some(message.content),
                            max_length: Some(2000),
                            min_length: None,
                            placeholder: None,
                            required: None,
                        })],
                    })])
                    .build(),
            ),
        })
        .await
    }

    pub async fn modal_submit(
        &self,
        mut modal: ModalSubmitInteraction,
    ) -> Result<(), anyhow::Error> {
        self.defer().await?;

        let channel = self
            .cache
            .channel(modal.channel_id)
            .context("channel not cached")?;
        let (channel_id, thread_id) = if channel.kind.is_thread() {
            (
                channel
                    .parent_id
                    .context("thread channel doesn't have a parent")?,
                Some(channel.id),
            )
        } else {
            (channel.id, None)
        };
        let input = modal
            .data
            .components
            .pop()
            .context("modal doesn't have any components")?
            .components
            .pop()
            .context("modal action row doesn't have any components")?;
        let webhook = self
            .webhooks_cache
            .get_infallible(&self.http, channel_id, "any message editor")
            .await?;
        let edit_message_id: Id<MessageMarker> = input.custom_id.parse()?;

        let mut reply = "done!";
        let unfiltered = self
            .cache
            .channel_messages(modal.channel_id)
            .context("channel messages aren't cached")?
            .take_while(|&id| id != edit_message_id)
            .chain([edit_message_id].into_iter())
            .map(|id| self.cache.message(id).context("message is not cached"))
            .collect::<Result<Vec<Reference<_, _>>, _>>()?;
        let messages: Vec<_> = unfiltered
            .iter()
            .rev()
            .filter(|m| {
                if cached_message_is_weird(m) {
                    reply = "done! there was a weird message sent after the message to edit so i \
                             left it alone";
                    false
                } else {
                    true
                }
            })
            .collect();
        for message in &messages {
            let author_id = message.author();
            let member = self
                .cache
                .member(
                    message
                        .guild_id()
                        .context("message doesn't have a guild id")?,
                    author_id,
                )
                .context("member is not cached")?;
            let user = self
                .cache
                .user(author_id)
                .context("message author user is not cached")?;

            let mut content = message.content().to_owned();
            #[allow(unused_must_use)]
            for attachment in message.attachments() {
                write!(content, "\n{}", attachment.url);
            }

            let minimal_member = MinimalMember::from_cached_member(&member, &user);
            let minimal_webhook = MinimalWebhook::try_from(webhook.value())?;
            let exec = minimal_webhook
                .execute_as_member(&self.http, thread_id, &minimal_member)?
                .content(&content)?;
            if message.id() == edit_message_id {
                let interaction_member = modal
                    .member
                    .as_ref()
                    .context("modal interaction doesn't have a member")?;
                exec.content(&input.value)?
                    .username(&format!(
                        "{} (edited by {})",
                        member.nick().unwrap_or(&user.name),
                        interaction_member.nick.as_ref().unwrap_or(
                            &interaction_member
                                .user
                                .as_ref()
                                .context("modal interaction member doesn't include user info")?
                                .name
                        )
                    ))?
                    .wait()
                    .exec()
                    .await?;
            } else {
                exec.wait().exec().await?;
            };
        }

        if messages.len() == 1 {
            self.http
                .delete_message(
                    modal.channel_id,
                    messages.first().context("list of messages is empty")?.id(),
                )
                .exec()
        } else {
            self.http
                .delete_messages(
                    modal.channel_id,
                    &messages.iter().map(|m| m.id()).collect::<Vec<_>>(),
                )
                .exec()
        }
        .await?;

        self.update_response().content(reply).exec().await?;

        Ok(())
    }
}

pub fn build() -> Command {
    CommandBuilder::new("edit".to_owned(), "".to_owned(), CommandType::Message)
        .default_member_permissions(Permissions::MANAGE_MESSAGES)
        .build()
}

fn message_is_weird(message: &Message) -> bool {
    message.activity.is_some()
        || message.application.is_some()
        || message.application_id.is_some()
        || message.author.bot
        || !message.components.is_empty()
        || !message.embeds.is_empty()
        || message.interaction.is_some()
        || !matches!(message.kind, MessageType::Regular | MessageType::Reply)
        || message.pinned
        || !message.reactions.is_empty()
        || !message.sticker_items.is_empty()
        || message.webhook_id.is_some()
}

fn cached_message_is_weird(message: &CachedMessage) -> bool {
    message.activity().is_some()
        || message.application().is_some()
        || message.application_id().is_some()
        || !message.components().is_empty()
        || !message.embeds().is_empty()
        || message.interaction().is_some()
        || !matches!(message.kind(), MessageType::Regular | MessageType::Reply)
        || message.pinned()
        || !message.reactions().is_empty()
        || !message.sticker_items().is_empty()
        || message.webhook_id().is_some()
}
