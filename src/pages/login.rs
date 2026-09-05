use iced::{
    Element, Length,
    widget::{TextInput, button, column, container, row, text, text_input},
};
use livekit_api::access_token::{AccessToken, VideoGrants};
use rust_i18n::t;

use crate::common::ApiKey;

#[derive(Debug, Default)]
pub struct LoginPage {
    api_key: String,
    api_secret: String,
    api_url: String,
    username: String,
    room_id: String,
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum LoginPageMessage {
    APIKeyContentChanged(String),
    APISecretContentChanged(String),
    APIURLContentChanged(String),
    UsernameContentChanged(String),
    RoomIDContentChanged(String),
    ConnectButtonPressed,
}

#[derive(Debug, Clone)]
pub enum LoginPageAction {
    Login { token: String, api_url: String },
    None,
}

impl LoginPage {
    pub fn default() -> Self {
        let api_key = ApiKey::from_env();
        Self {
            api_key: api_key.api_key,
            api_secret: api_key.api_secret,
            api_url: api_key.api_url,
            username: api_key.identity,
            room_id: api_key.room,
            error: None,
        }
    }

    pub fn view<'a>(&'a self) -> Element<'a, LoginPageMessage> {
        let field = |label_key: &str, input: TextInput<'a, LoginPageMessage>| {
            column![text(t!(label_key).into_owned()).size(14), input].spacing(4)
        };

        column![
            row![
                column![
                    field(
                        "login.api_key",
                        text_input("", &self.api_key)
                            .on_input(LoginPageMessage::APIKeyContentChanged),
                    ),
                    field(
                        "login.api_secret",
                        text_input("", &self.api_secret)
                            .on_input(LoginPageMessage::APISecretContentChanged)
                            .secure(true),
                    ),
                    field(
                        "login.api_url",
                        text_input("", &self.api_url)
                            .on_input(LoginPageMessage::APIURLContentChanged),
                    ),
                ]
                .spacing(10)
                .padding(20),
                column![
                    field(
                        "login.username",
                        text_input("", &self.username)
                            .on_input(LoginPageMessage::UsernameContentChanged),
                    ),
                    field(
                        "login.room_id",
                        text_input("", &self.room_id)
                            .on_input(LoginPageMessage::RoomIDContentChanged),
                    ),
                ]
                .spacing(10)
                .padding(20),
            ]
            .spacing(10),
        ]
        .extend(self.error.iter().map(|error| {
            container(text(error).style(text::danger))
                .padding([0, 20])
                .width(Length::Fill)
                .into()
        }))
        .push(
            container(
                button(text(t!("login.connect_btn")))
                    .on_press(LoginPageMessage::ConnectButtonPressed),
            )
            .padding(20)
            .height(Length::Fill)
            .width(Length::Fill)
            .align_right(Length::Fill),
        )
        .into()
    }

    pub fn update(&mut self, message: LoginPageMessage) -> LoginPageAction {
        match message {
            LoginPageMessage::APIKeyContentChanged(value) => self.api_key = value,
            LoginPageMessage::APISecretContentChanged(value) => self.api_secret = value,
            LoginPageMessage::APIURLContentChanged(value) => self.api_url = value,
            LoginPageMessage::UsernameContentChanged(value) => self.username = value,
            LoginPageMessage::RoomIDContentChanged(value) => self.room_id = value,
            LoginPageMessage::ConnectButtonPressed => {
                let auth_data = ApiKey {
                    api_key: self.api_key.clone(),
                    api_secret: self.api_secret.clone(),
                    api_url: self.api_url.clone(),
                    identity: self.username.clone(),
                    room: self.room_id.clone(),
                };

                return match get_access_token(&auth_data) {
                    Ok(token) => {
                        self.error = None;
                        LoginPageAction::Login {
                            token,
                            api_url: auth_data.api_url,
                        }
                    }
                    Err(error) => {
                        self.error = Some(error);
                        LoginPageAction::None
                    }
                };
            }
        }

        LoginPageAction::None
    }
}

fn get_access_token(auth_data: &ApiKey) -> Result<String, String> {
    let ApiKey {
        api_key,
        api_secret,
        api_url: _,
        identity,
        room,
    } = auth_data;
    AccessToken::with_api_key(api_key, api_secret)
        .with_identity(identity)
        .with_name(identity)
        .with_grants(VideoGrants {
            room_join: true,
            room: room.clone(),
            ..Default::default()
        })
        .to_jwt()
        .map_err(|e| e.to_string())
}
