pub mod edit;

use std::{mem, ops::Deref};

use anyhow::{anyhow, IntoResult};
use thiserror::Error;
use twilight_http::Client;
use twilight_interactions::command::CreateCommand;
use twilight_model::{
    application::{
        component::Component,
        interaction::{
            modal::ModalSubmitInteraction, ApplicationCommand, Interaction, InteractionType,
            MessageComponentInteraction,
        },
    },
    channel::message::MessageFlags,
    guild::{PartialMember, Permissions},
    http::interaction::{InteractionResponse, InteractionResponseData, InteractionResponseType},
    id::{
        marker::{ApplicationMarker, ChannelMarker, GuildMarker, InteractionMarker},
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

struct UpdateResponse<'res> {
    handler: &'res Handler<'res>,
    content: Option<&'res str>,
    components: Option<&'res [Component]>,
}

impl<'res> UpdateResponse<'res> {
    async fn exec(&self) -> Result<(), anyhow::Error> {
        self.handler
            .http
            .interaction(self.handler.application_id)
            .update_response(&self.handler.token)
            .content(self.content)?
            .components(self.components)?
            .exec()
            .await?;

        Ok(())
    }

    const fn content(mut self, content: &'res str) -> Self {
        self.content = Some(content);
        self
    }

    const fn components(mut self, components: &'res [Component]) -> Self {
        self.components = Some(components);
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
            Interaction::MessageComponent(component) => {
                (mem::take(&mut component.token), component.id)
            }
            Interaction::ModalSubmit(modal) => (mem::take(&mut modal.token), modal.id),
            _ => return Err(anyhow!("type of the interaction to handle is unknown")),
        };

        Ok(Self { ctx, id, token })
    }

    #[allow(clippy::wildcard_enum_match_arm, clippy::option_if_let_else)]
    pub async fn handle(&self, interaction: Interaction) -> Result<(), anyhow::Error> {
        self.defer(interaction.kind()).await?;

        if let Err(err) = match interaction {
            Interaction::ApplicationCommand(cmd) => self.handle_command(*cmd).await,
            Interaction::MessageComponent(component) => self.handle_component(*component).await,
            Interaction::ModalSubmit(modal) => self.handle_modal_submit(*modal).await,
            _ => return Err(anyhow!("unknown interaction: {interaction:#?}")),
        } {
            return if let Some(user_err) = err.downcast_ref::<Error>() {
                self.update_response()
                    .content(&user_err.to_string())
                    .components(&[])
                    .exec()
                    .await?;
                Ok(())
            } else {
                self.update_response()
                    .content(
                        "an error happened :( i let my developer know hopefully they'll fix it \
                         soon!",
                    )
                    .components(&[])
                    .exec()
                    .await?;
                Err(err)
            };
        };

        Ok(())
    }

    async fn handle_command(&self, command: ApplicationCommand) -> Result<(), anyhow::Error> {
        match command.data.name.as_str() {
            "edit" => self.edit().command(command).await,
            _ => Err(anyhow!("unknown command: {command:#?}")),
        }
    }

    async fn handle_component(
        &self,
        component: MessageComponentInteraction,
    ) -> Result<(), anyhow::Error> {
        match component.data.custom_id.as_str() {
            "selected_message" => self.edit().message_select(component).await,
            _ => Err(anyhow!("unknown component: {component:#?}")),
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
    async fn defer(&self, kind: InteractionType) -> Result<(), anyhow::Error> {
        let response_type = match kind {
            InteractionType::ApplicationCommand => {
                InteractionResponseType::DeferredChannelMessageWithSource
            }
            InteractionType::ModalSubmit => InteractionResponseType::DeferredUpdateMessage,
            _ => return Ok(()),
        };

        self.http
            .interaction(self.application_id)
            .create_response(
                self.id,
                &self.token,
                &InteractionResponse {
                    kind: response_type,
                    data: Some(InteractionResponseData {
                        flags: Some(MessageFlags::EPHEMERAL),
                        ..InteractionResponseData::default()
                    }),
                },
            )
            .exec()
            .await?;

        Ok(())
    }

    const fn update_response(&self) -> UpdateResponse<'_> {
        UpdateResponse {
            handler: self,
            content: None,
            components: None,
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
    let commands = [edit::Command::create_command().into()];

    match test_guild_id {
        Some(id) => interaction_client.set_guild_commands(id, &commands).exec(),
        None => interaction_client.set_global_commands(&commands).exec(),
    }
    .await?;

    Ok(())
}

fn check_member_permissions(
    member: &PartialMember,
    required: Permissions,
) -> Result<(), anyhow::Error> {
    let missing_permissions = required - member.permissions.ok()?;

    if missing_permissions.is_empty() {
        Ok(())
    } else {
        Err(Error::UserMissingPermissions(missing_permissions).into())
    }
}
