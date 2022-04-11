mod edit;

use anyhow::{bail, Result};
use twilight_model::{
    application::interaction::Interaction,
    http::interaction::{InteractionResponse, InteractionResponseType},
};

use crate::Context;

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
