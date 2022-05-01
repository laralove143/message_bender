use std::ops::Deref;

use anyhow::Ok;
use thiserror::Error;
use twilight_interactions::command::{CommandModel, CreateCommand};
use twilight_model::{
    application::{
        component::{
            select_menu::SelectMenuOption, text_input::TextInputStyle, ActionRow, Component,
            SelectMenu, TextInput,
        },
        interaction::{
            modal::ModalSubmitInteraction, ApplicationCommand, MessageComponentInteraction,
        },
    },
    guild::Permissions,
    http::interaction::{InteractionResponse, InteractionResponseType},
    id::{marker::MessageMarker, Id},
};
use twilight_util::builder::InteractionResponseDataBuilder;
use twilight_webhook::util::{MinimalMember, MinimalWebhook};

use crate::{interaction, interaction::check_member_permissions};

#[derive(Error, Debug)]
pub enum Error {
    #[error(
        "i dont know any messages here yet.. i can only see messages sent after i joined.. sorry!"
    )]
    NoCachedMessages,
}

#[derive(CreateCommand, CommandModel)]
#[command(name = "edit", desc = "edit any message you select")]
pub struct Command {}

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

    pub async fn command(&self, command: ApplicationCommand) -> Result<(), anyhow::Error> {
        self.check_self_permissions(
            command.channel_id,
            Permissions::MANAGE_MESSAGES | Permissions::MANAGE_WEBHOOKS | Permissions::VIEW_CHANNEL,
        )?;
        check_member_permissions(&command.member.ok()?, Permissions::MANAGE_MESSAGES)?;

        let mut message_options: Vec<SelectMenuOption> = Vec::with_capacity(25);
        let mut messages = self
            .cache
            .channel_messages(command.channel_id)
            .map(Iterator::peekable);
        if messages.as_mut().map_or(true, |msgs| msgs.peek().is_none()) {
            return Err(super::Error::Edit(Error::NoCachedMessages).into());
        }

        for id in messages.ok()? {
            let message = self.cache.message(id).ok()?;
            let content = message.content();
            if content.len() >= 2000 {
                continue;
            }
            message_options.push(SelectMenuOption {
                label: content
                    .get(0..100)
                    .or_else(|| content.get(0..99))
                    .or_else(|| content.get(0..98))
                    .or_else(|| content.get(0..97))
                    .or_else(|| content.get(0..96))
                    .unwrap_or(content)
                    .to_owned(),
                value: id.to_string(),
                default: false,
                description: None,
                emoji: None,
            });
        }

        self.update_response()
            .content("please select the message you want to edit")
            .components(&[Component::ActionRow(ActionRow {
                components: vec![Component::SelectMenu(SelectMenu {
                    custom_id: "selected_message".to_owned(),
                    options: message_options,
                    placeholder: Some("message to edit".to_owned()),
                    disabled: false,
                    max_values: None,
                    min_values: None,
                })],
            })])
            .exec()
            .await?;

        Ok(())
    }

    pub async fn message_select(
        &self,
        mut component: MessageComponentInteraction,
    ) -> Result<(), anyhow::Error> {
        let selected_message = self
            .cache
            .message(component.data.values.pop().ok()?.parse()?)
            .ok()?;

        self.create_response(&InteractionResponse {
            kind: InteractionResponseType::Modal,
            data: Some(
                InteractionResponseDataBuilder::new()
                    .title("edit message".to_owned())
                    .custom_id("edit_modal".to_owned())
                    .components([Component::ActionRow(ActionRow {
                        components: vec![Component::TextInput(TextInput {
                            custom_id: selected_message.id().to_string(),
                            label: "what to edit the message to".to_owned(),
                            style: TextInputStyle::Paragraph,
                            value: Some(selected_message.content().to_owned()),
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
        let channel = self.cache.channel(modal.channel_id).ok()?;
        let (channel_id, thread_id) = if channel.kind.is_thread() {
            (channel.parent_id.ok()?, Some(channel.id))
        } else {
            (channel.id, None)
        };
        let input = modal.data.components.pop().ok()?.components.pop().ok()?;
        let webhook = self
            .webhooks_cache
            .get_infallible(&self.http, channel_id, "any message editor")
            .await?;
        let edit_message_id: Id<MessageMarker> = input.custom_id.parse()?;
        let message_ids: Vec<Id<MessageMarker>> = self
            .cache
            .channel_messages(modal.channel_id)
            .ok()?
            .take_while(|&id| id != edit_message_id)
            .chain([edit_message_id].into_iter())
            .collect();

        for id in message_ids.iter().rev() {
            let message = self.cache.message(*id).ok()?;
            if message.webhook_id().is_some() {
                continue;
            }
            let author_id = message.author();
            MinimalWebhook::try_from(webhook.value())?
                .execute_as_member(
                    &self.http,
                    thread_id,
                    &MinimalMember::from((
                        &*self
                            .cache
                            .member(message.guild_id().ok()?, author_id)
                            .ok()?,
                        &*self.cache.user(author_id).ok()?,
                    )),
                )
                .content(if id == &edit_message_id {
                    &input.value
                } else {
                    message.content()
                })?
                .exec()
                .await?;
        }

        if message_ids.len() == 1 {
            self.http
                .delete_message(modal.channel_id, *message_ids.first().ok()?)
                .exec()
        } else {
            self.http
                .delete_messages(modal.channel_id, &message_ids)
                .exec()
        }
        .await?;

        self.update_response()
            .content("done!")
            .components(&[])
            .exec()
            .await?;

        Ok(())
    }
}
