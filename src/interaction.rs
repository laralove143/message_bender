mod edit;

use std::mem;

use anyhow::anyhow;
use thiserror::Error;
use twilight_interactions::command::CreateCommand;
use twilight_model::{
    application::interaction::{
        modal::ModalSubmitInteraction, ApplicationCommand, Interaction, MessageComponentInteraction,
    },
    channel::message::MessageFlags,
    guild::Permissions,
    http::interaction::{InteractionResponse, InteractionResponseType},
    id::{
        marker::{ChannelMarker, GuildMarker, UserMarker},
        Id,
    },
};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::Context;

#[derive(Error, Debug)]
enum Error {
    #[error("{0}")]
    Edit(#[from] edit::Error),
    #[error("you don't have these required permissions:\n**{}**",
    format!("{:#?}", .0).to_lowercase().replace('_', " "))]
    UserMissingPermissions(Permissions),
    #[error("please give me these permissions first:\n**{}**",
    format!("{:#?}", .0).to_lowercase().replace('_', " "))]
    SelfMissingPermissions(Permissions),
}

impl Context {
    #[allow(clippy::wildcard_enum_match_arm)]
    pub async fn handle_interaction(&self, interaction: Interaction) -> Result<(), anyhow::Error> {
        let (id, token, result) = match interaction {
            Interaction::ApplicationCommand(mut cmd) => {
                (cmd.id, mem::take(&mut cmd.token), self.handle_command(*cmd))
            }
            Interaction::MessageComponent(mut component) => (
                component.id,
                mem::take(&mut component.token),
                self.handle_component(*component),
            ),
            Interaction::ModalSubmit(mut modal) => (
                modal.id,
                mem::take(&mut modal.token),
                self.handle_modal_submit(*modal),
            ),
            _ => {
                return Err(anyhow!("unknown interaction: {interaction:#?}"));
            }
        };

        match result {
            Ok(response) => {
                self.http
                    .interaction(self.application_id)
                    .create_response(id, &token, &response)
                    .exec()
                    .await?;

                Ok(())
            }
            Err(err) => {
                if let Some(user_err) = err.downcast_ref::<Error>() {
                    self.http
                        .interaction(self.application_id)
                        .create_response(
                            id,
                            &token,
                            &InteractionResponse {
                                kind: InteractionResponseType::ChannelMessageWithSource,
                                data: Some(
                                    InteractionResponseDataBuilder::new()
                                        .content(user_err.to_string())
                                        .flags(MessageFlags::EPHEMERAL)
                                        .build(),
                                ),
                            },
                        )
                        .exec()
                        .await?;

                    Ok(())
                } else {
                    self.http
                        .interaction(self.application_id)
                        .create_response(
                            id,
                            &token,
                            &InteractionResponse {
                                kind: InteractionResponseType::ChannelMessageWithSource,
                                data: Some(
                                    InteractionResponseDataBuilder::new()
                                        .content(
                                            "an error happened :( i let my developer know \
                                             hopefully they'll fix it soon!"
                                                .to_owned(),
                                        )
                                        .flags(MessageFlags::EPHEMERAL)
                                        .build(),
                                ),
                            },
                        )
                        .exec()
                        .await?;

                    Err(err)
                }
            }
        }
    }

    fn handle_command(
        &self,
        command: ApplicationCommand,
    ) -> Result<InteractionResponse, anyhow::Error> {
        match command.data.name.as_str() {
            "edit" => self.edit_runner().handle_command(command),
            _ => Err(anyhow!("unknown command: {command:#?}")),
        }
    }

    fn handle_component(
        &self,
        component: MessageComponentInteraction,
    ) -> Result<InteractionResponse, anyhow::Error> {
        match component.data.custom_id.as_str() {
            "selected_message" => self.edit_runner().handle_message_select(component),
            _ => Err(anyhow!("unknown component: {component:#?}")),
        }
    }

    fn handle_modal_submit(
        &self,
        modal: ModalSubmitInteraction,
    ) -> Result<InteractionResponse, anyhow::Error> {
        match modal.data.custom_id.as_str() {
            "edit_modal" => self.edit_runner().handle_modal_submit(modal),
            _ => Err(anyhow!("unknown modal: {modal:#?}")),
        }
    }

    fn check_user_permissions(
        &self,
        user_id: Id<UserMarker>,
        channel_id: Id<ChannelMarker>,
        required: Permissions,
    ) -> Result<(), anyhow::Error> {
        let missing_permissions =
            required - self.cache.permissions().in_channel(user_id, channel_id)?;

        if missing_permissions.is_empty() {
            Ok(())
        } else {
            Err(Error::UserMissingPermissions(missing_permissions).into())
        }
    }

    fn check_self_permissions(
        &self,
        channel_id: Id<ChannelMarker>,
        required: Permissions,
    ) -> Result<(), anyhow::Error> {
        let missing_permissions = required
            - self
                .cache
                .permissions()
                .in_channel(self.user_id, channel_id)?;

        if missing_permissions.is_empty() {
            Ok(())
        } else {
            Err(Error::SelfMissingPermissions(missing_permissions).into())
        }
    }

    pub async fn create_commands(
        &self,
        test_guild_id: Option<Id<GuildMarker>>,
    ) -> Result<(), anyhow::Error> {
        let interaction_client = self.http.interaction(self.application_id);
        let commands = [edit::Command::create_command().into()];
        match test_guild_id {
            Some(id) => interaction_client.set_guild_commands(id, &commands).exec(),
            None => interaction_client.set_global_commands(&commands).exec(),
        }
        .await?;

        Ok(())
    }
}
