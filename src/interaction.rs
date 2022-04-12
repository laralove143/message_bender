mod edit;

use anyhow::anyhow;
use thiserror::Error;
use twilight_interactions::command::CreateCommand;
use twilight_model::{
    application::interaction::Interaction,
    channel::message::MessageFlags,
    http::interaction::{InteractionResponse, InteractionResponseType},
    id::{marker::GuildMarker, Id},
};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::{interaction::edit::Edit, Context};

#[derive(Error, Debug)]
pub enum Error {
    #[error(
        "i dont know any messages here yet.. i can only see messages sent after i joined.. sorry!"
    )]
    NoCachedMessages,
    #[error("an error happened :( i let my developer know hopefully they'll fix it soon!")]
    Other(#[from] anyhow::Error),
}

impl Error {
    fn response(&self) -> InteractionResponse {
        InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(
                InteractionResponseDataBuilder::new()
                    .content(self.to_string())
                    .flags(MessageFlags::EPHEMERAL)
                    .build(),
            ),
        }
    }
}

impl Context {
    pub async fn handle_interaction(&self, interaction: Interaction) -> Result<(), anyhow::Error> {
        let command = if let Interaction::ApplicationCommand(cmd) = interaction {
            *cmd
        } else {
            return Err(anyhow!("unknown interaction: {interaction:#?}"));
        };

        match match command.data.name.as_str() {
            "edit" => self.handle_edit_command(&command).await,
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
                self.http
                    .interaction(self.application_id)
                    .create_response(command.id, &command.token, &err.response())
                    .exec()
                    .await?;

                Err(err.into())
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
