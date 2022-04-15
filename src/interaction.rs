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
        marker::{ChannelMarker, GuildMarker, InteractionMarker, UserMarker},
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
    #[allow(clippy::wildcard_enum_match_arm, clippy::option_if_let_else)]
    pub async fn handle_interaction(
        &self,
        mut interaction: Interaction,
    ) -> Result<(), anyhow::Error> {
        let (deferred, token, id) = self.defer(&mut interaction).await?;

        let result = match interaction {
            Interaction::ApplicationCommand(cmd) => self.handle_command(*cmd),
            Interaction::MessageComponent(component) => self.handle_component(*component),
            Interaction::ModalSubmit(modal) => self.handle_modal_submit(*modal).await,
            _ => {
                return Err(anyhow!("unknown interaction: {interaction:#?}"));
            }
        };

        if deferred {}
        let (response, result) = match result {
            Ok(response) => (response, Ok(())),
            Err(err) => {
                if let Some(user_err) = err.downcast_ref::<Error>() {
                    (
                        InteractionResponse {
                            kind: InteractionResponseType::ChannelMessageWithSource,
                            data: Some(
                                InteractionResponseDataBuilder::new()
                                    .content(user_err.to_string())
                                    .flags(MessageFlags::EPHEMERAL)
                                    .build(),
                            ),
                        },
                        Ok(()),
                    )
                } else {
                    (
                        InteractionResponse {
                            kind: InteractionResponseType::ChannelMessageWithSource,
                            data: Some(
                                InteractionResponseDataBuilder::new()
                                    .content(
                                        "an error happened :( i let my developer know hopefully \
                                         they'll fix it soon!"
                                            .to_owned(),
                                    )
                                    .flags(MessageFlags::EPHEMERAL)
                                    .build(),
                            ),
                        },
                        Err(err),
                    )
                }
            }
        };

        let client = self.http.interaction(self.application_id);
        if deferred {
            client.update_response(token)
        } else {
            client.create_response(id, &token, &response).exec().await?;
        }

        result
    }

    #[allow(clippy::wildcard_enum_match_arm)]
    async fn defer(
        &self,
        interaction: &mut Interaction,
    ) -> Result<(bool, String, Id<InteractionMarker>), anyhow::Error> {
        let mut deferred = false;

        let (token, id) = match interaction {
            Interaction::ApplicationCommand(cmd) => (mem::take(&mut cmd.token), cmd.id),
            Interaction::MessageComponent(component) => {
                (mem::take(&mut component.token), component.id)
            }
            Interaction::ModalSubmit(modal) => (mem::take(&mut modal.token), modal.id),
            _ => return Err(anyhow!("deferred interaction type unknown")),
        };

        if let Interaction::ModalSubmit(modal) = interaction {
            if modal.data.custom_id == "edit_modal" {
                deferred = true;
                self.http
                    .interaction(self.application_id)
                    .create_response(
                        id,
                        &token,
                        &InteractionResponse {
                            kind: InteractionResponseType::DeferredUpdateMessage,
                            data: None,
                        },
                    )
                    .exec()
                    .await?;
            }
        }

        Ok((deferred, token, id))
    }

    fn handle_command(
        &self,
        command: ApplicationCommand,
    ) -> Result<InteractionResponse, anyhow::Error> {
        match command.data.name.as_str() {
            "edit" => self.edit_runner().command(command),
            _ => Err(anyhow!("unknown command: {command:#?}")),
        }
    }

    fn handle_component(
        &self,
        component: MessageComponentInteraction,
    ) -> Result<InteractionResponse, anyhow::Error> {
        match component.data.custom_id.as_str() {
            "selected_message" => self.edit_runner().message_select(component),
            _ => Err(anyhow!("unknown component: {component:#?}")),
        }
    }

    async fn handle_modal_submit(
        &self,
        modal: ModalSubmitInteraction,
    ) -> Result<InteractionResponse, anyhow::Error> {
        match modal.data.custom_id.as_str() {
            "edit_modal" => self.edit_runner().modal_submit(token, modal).await,
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
