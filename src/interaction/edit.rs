use anyhow::Ok;
use thiserror::Error;
use twilight_interactions::command::{CommandModel, CreateCommand};
use twilight_model::{
    application::{
        component::{select_menu::SelectMenuOption, ActionRow, Component, SelectMenu},
        interaction::ApplicationCommand,
    },
    channel::message::MessageFlags,
    guild::Permissions,
    http::interaction::{InteractionResponse, InteractionResponseType},
};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::Context;

#[derive(Error, Debug)]
pub enum Error {
    #[error(
        "i dont know any messages here yet.. i can only see messages sent after i joined.. sorry!"
    )]
    NoCachedMessages,
}

#[derive(CreateCommand, CommandModel)]
#[command(name = "edit", desc = "edit any message you select")]
pub struct Edit {}

impl Context {
    pub fn handle_edit_command(
        &self,
        command: &ApplicationCommand,
    ) -> Result<InteractionResponse, anyhow::Error> {
        self.check_permissions(
            command.member.as_ref().ok()?.user.as_ref().ok()?.id,
            command.channel_id,
            Permissions::MANAGE_MESSAGES,
        )?;

        let mut message_options: Vec<SelectMenuOption> = Vec::with_capacity(25);
        for id in self
            .cache
            .channel_messages(command.channel_id)
            .ok_or(super::Error::Edit(Error::NoCachedMessages))?
        {
            let message = self.cache.message(id).ok()?;
            let content = message.content();
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

        Ok(InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(
                InteractionResponseDataBuilder::new()
                    .content("please select the message you want to edit".to_owned())
                    .components([Component::ActionRow(ActionRow {
                        components: vec![Component::SelectMenu(SelectMenu {
                            custom_id: "message".to_owned(),
                            options: message_options,
                            placeholder: Some("message to edit".to_owned()),
                            disabled: false,
                            max_values: None,
                            min_values: None,
                        })],
                    })])
                    .flags(MessageFlags::EPHEMERAL)
                    .build(),
            ),
        })
    }
}
