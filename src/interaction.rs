mod edit;

use std::mem;

use anyhow::anyhow;
use thiserror::Error;
use twilight_interactions::command::CreateCommand;
use twilight_model::{
    application::interaction::{
        modal::ModalSubmitInteraction, ApplicationCommand, Interaction, MessageComponentInteraction,
    },
    guild::Permissions,
    http::interaction::{InteractionResponse, InteractionResponseType},
    id::{
        marker::{ChannelMarker, GuildMarker, UserMarker},
        Id,
    },
};

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
        let token = self.defer(&mut interaction).await?;

        let client = self.http.interaction(self.application_id);
        if let Err(err) = match interaction {
            Interaction::ApplicationCommand(cmd) => self.handle_command(*cmd),
            Interaction::MessageComponent(component) => self.handle_component(*component),
            Interaction::ModalSubmit(modal) => self.handle_modal_submit(*modal).await,
            _ => return Err(anyhow!("unknown interaction: {interaction:#?}")),
        } {
            return if let Some(user_err) = err.downcast_ref::<Error>() {
                client
                    .update_response(&token)
                    .content(Some(&user_err.to_string()))?
                    .exec()
                    .await?;
                Ok(())
            } else {
                client
                    .update_response(&token)
                    .content(Some(
                        "an error happened :( i let my developer know hopefully they'll fix it \
                         soon!",
                    ))?
                    .exec()
                    .await?;
                Err(err)
            };
        };

        Ok(())
    }

    #[allow(clippy::wildcard_enum_match_arm)]
    async fn defer(&self, interaction: &mut Interaction) -> Result<String, anyhow::Error> {
        let (token, id, response_type) = match interaction {
            Interaction::ApplicationCommand(cmd) => (
                mem::take(&mut cmd.token),
                cmd.id,
                InteractionResponseType::DeferredChannelMessageWithSource,
            ),
            Interaction::MessageComponent(component) => (
                mem::take(&mut component.token),
                component.id,
                InteractionResponseType::DeferredUpdateMessage,
            ),
            Interaction::ModalSubmit(modal) => (
                mem::take(&mut modal.token),
                modal.id,
                InteractionResponseType::DeferredUpdateMessage,
            ),
            _ => return Err(anyhow!("type of the interaction to defer is unknown")),
        };

        self.http
            .interaction(self.application_id)
            .create_response(
                id,
                &token,
                &InteractionResponse {
                    kind: response_type,
                    data: None,
                },
            )
            .exec()
            .await?;

        Ok(token)
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
        _modal: ModalSubmitInteraction,
    ) -> Result<InteractionResponse, anyhow::Error> {
        // match modal.data.custom_id.as_str() {
        //     "edit_modal" => self.edit_runner().modal_submit(token, modal).await,
        //     _ => Err(anyhow!("unknown modal: {modal:#?}")),
        // }
        Ok(InteractionResponse {
            kind: InteractionResponseType::Pong,
            data: None,
        })
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
