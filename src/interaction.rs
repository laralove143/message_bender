mod edit;

use anyhow::{bail, Result};
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
    fn response(self) -> InteractionResponse {
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
    pub async fn handle_interaction(&self, interaction: Interaction) -> Result<()> {
        let client = self.http.interaction(self.application_id);

        let command = if let Interaction::ApplicationCommand(cmd) = interaction {
            *cmd
        } else {
            bail!("unknown interaction: {interaction:#?}");
        };

        let response = match command.data.name.as_str() {
            "edit" => self.handle_edit_command(&command).await,
            _ => bail!("unknown command: {command:#?}"),
        };

        client
            .create_response(
                command.id,
                &command.token,
                &InteractionResponse {
                    kind: InteractionResponseType::ChannelMessageWithSource,
                    data: Some(response_data),
                },
            )
            .exec()
            .await?;

        Ok(())
    }
}
