pub mod edit;

use std::{mem, ops::Deref};

use anyhow::anyhow;
use thiserror::Error;
use twilight_http::Client;
use twilight_interactions::command::CreateCommand;
use twilight_model::{
    application::{
        command::CommandType,
        interaction::{modal::ModalSubmitInteraction, ApplicationCommand, Interaction},
    },
    channel::message::MessageFlags,
    guild::Permissions,
    http::interaction::{InteractionResponse, InteractionResponseType},
    id::{
        marker::{ApplicationMarker, ChannelMarker, GuildMarker, InteractionMarker},
        Id,
    },
};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::Context;

#[derive(Error, Debug)]
enum Error {
    #[error("{0}")]
    Edit(#[from] edit::Error),
    #[error("please give me these permissions first:\n**{}**",
    format!("{:#?}", .0).to_lowercase().replace('_', " "))]
    SelfMissingPermissions(Permissions),
}

struct UpdateResponse<'res> {
    handler: &'res Handler<'res>,
    content: Option<&'res str>,
}

impl<'res> UpdateResponse<'res> {
    async fn exec(&self) -> Result<(), anyhow::Error> {
        self.handler
            .http
            .interaction(self.handler.application_id)
            .update_response(&self.handler.token)
            .content(self.content)?
            .exec()
            .await?;

        Ok(())
    }

    const fn content(mut self, content: &'res str) -> Self {
        self.content = Some(content);
        self
    }
}

pub struct Handler<'ctx> {
    ctx: &'ctx Context,
    id: Id<InteractionMarker>,
    token: String,
}

impl Deref for Handler<'_> {
    type Target = Context;

    fn deref(&self) -> &Self::Target {
        self.ctx
    }
}

impl<'ctx> Handler<'ctx> {
    #[allow(clippy::wildcard_enum_match_arm)]
    pub fn new(
        ctx: &'ctx Context,
        interaction: &mut Interaction,
    ) -> Result<Handler<'ctx>, anyhow::Error> {
        let (token, id) = match interaction {
            Interaction::ApplicationCommand(cmd) => (mem::take(&mut cmd.token), cmd.id),
            Interaction::ModalSubmit(modal) => (mem::take(&mut modal.token), modal.id),
            _ => return Err(anyhow!("unknown interaction type: {interaction:#?}")),
        };

        Ok(Self { ctx, id, token })
    }

    #[allow(clippy::wildcard_enum_match_arm, clippy::option_if_let_else)]
    pub async fn handle(&self, interaction: Interaction) -> Result<(), anyhow::Error> {
        if let Err(err) = match interaction {
            Interaction::ApplicationCommand(cmd) => self.handle_command(*cmd).await,
            Interaction::ModalSubmit(modal) => self.handle_modal_submit(*modal).await,
            _ => return Err(anyhow!("unknown interaction type: {interaction:#?}")),
        } {
            return if let Some(user_err) = err.downcast_ref::<Error>() {
                self.update_response()
                    .content(&user_err.to_string())
                    .exec()
                    .await?;
                Ok(())
            } else {
                self.update_response()
                    .content(
                        "an error happened :( i let my developer know hopefully they'll fix it \
                         soon!",
                    )
                    .exec()
                    .await?;
                Err(err)
            };
        };

        Ok(())
    }

    async fn handle_command(&self, command: ApplicationCommand) -> Result<(), anyhow::Error> {
        match command.data.name.as_str() {
            "edit" => match command.data.kind {
                CommandType::Message => self.edit().command(command).await,
                CommandType::ChatInput => self.edit().chat_input_command().await,
                CommandType::User => Err(anyhow!("unknown command type: {command:#?}")),
            },
            _ => Err(anyhow!("unknown command: {command:#?}")),
        }
    }

    async fn handle_modal_submit(
        &self,
        modal: ModalSubmitInteraction,
    ) -> Result<(), anyhow::Error> {
        match modal.data.custom_id.as_str() {
            "edit_modal" => self.edit().modal_submit(modal).await,
            _ => Err(anyhow!("unknown modal: {modal:#?}")),
        }
    }

    #[allow(clippy::wildcard_enum_match_arm)]
    async fn defer(&self) -> Result<(), anyhow::Error> {
        self.create_response(&InteractionResponse {
            kind: InteractionResponseType::DeferredChannelMessageWithSource,
            data: Some(
                InteractionResponseDataBuilder::new()
                    .flags(MessageFlags::EPHEMERAL)
                    .build(),
            ),
        })
        .await?;

        Ok(())
    }

    const fn update_response(&self) -> UpdateResponse<'_> {
        UpdateResponse {
            handler: self,
            content: None,
        }
    }

    async fn create_response(&self, response: &InteractionResponse) -> Result<(), anyhow::Error> {
        self.http
            .interaction(self.application_id)
            .create_response(self.id, &self.token, response)
            .exec()
            .await?;

        Ok(())
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

    pub const fn edit(&self) -> edit::Handler {
        edit::Handler::new(self)
    }
}

pub async fn create_commands(
    http: &Client,
    application_id: Id<ApplicationMarker>,
    test_guild_id: Option<Id<GuildMarker>>,
) -> Result<(), anyhow::Error> {
    let interaction_client = http.interaction(application_id);
    let commands = [edit::build(), edit::ChatInput::create_command().into()];

    match test_guild_id {
        Some(id) => interaction_client.set_guild_commands(id, &commands).exec(),
        None => interaction_client.set_global_commands(&commands).exec(),
    }
    .await?;

    Ok(())
}
