mod edit;

use anyhow::anyhow;
use thiserror::Error;
use twilight_interactions::command::CreateCommand;
use twilight_model::{
    application::interaction::Interaction,
    channel::message::MessageFlags,
    guild::Permissions,
    http::interaction::{InteractionResponse, InteractionResponseType},
    id::{
        marker::{ChannelMarker, GuildMarker, UserMarker},
        Id,
    },
};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::{interaction::edit::Edit, Context};

#[derive(Error, Debug)]
enum Error {
    #[error("{0}")]
    Edit(#[from] edit::Error),
    #[error("you need these permissions for that: ```{0:#?}```")]
    UserMissingPermissions(Permissions),
}

impl Context {
    pub async fn handle_interaction(&self, interaction: Interaction) -> Result<(), anyhow::Error> {
        let command = if let Interaction::ApplicationCommand(cmd) = interaction {
            *cmd
        } else {
            return Err(anyhow!("unknown interaction: {interaction:#?}"));
        };

        match match command.data.name.as_str() {
            "edit" => self.handle_edit_command(&command),
            _ => return Err(anyhow!("unknown command: {command:#?}")),
        } {
            Ok(response) => {
                self.http
                    .interaction(self.application_id)
                    .create_response(command.id, &command.token, &response)
                    .exec()
                    .await?;

                Ok(())
            }
            Err(err) => {
                if let Some(user_err) = err.downcast_ref::<Error>() {
                    self.http
                        .interaction(self.application_id)
                        .create_response(
                            command.id,
                            &command.token,
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
                            command.id,
                            &command.token,
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

    pub async fn create_commands(
        &self,
        test_guild_id: Option<Id<GuildMarker>>,
    ) -> Result<(), anyhow::Error> {
        let interaction_client = self.http.interaction(self.application_id);
        let commands = [Edit::create_command().into()];
        match test_guild_id {
            Some(id) => interaction_client.set_guild_commands(id, &commands).exec(),
            None => interaction_client.set_global_commands(&commands).exec(),
        }
        .await?;

        Ok(())
    }
}
