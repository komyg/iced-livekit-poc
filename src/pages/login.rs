use iced::{
    Element, Length,
    widget::{button, column, container, row, text_input},
};
use livekit_api::access_token::{AccessToken, VideoGrants};

use crate::common::ApiKey;

#[derive(Debug, Default)]
pub struct LoginPage {
    pub api_key: String,
    pub api_secret: String,
    pub api_url: String,
    pub username: String,
    pub room_id: String,
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
    Connect { token: String, api_url: String },
    Failed(String),
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
        }
    }

    pub fn view(&self) -> Element<'_, LoginPageMessage> {
        column![
            row![
                column![
                    text_input("Api Key", &self.api_key)
                        .on_input(LoginPageMessage::APIKeyContentChanged),
                    text_input("Api Secret", &self.api_secret)
                        .on_input(LoginPageMessage::APISecretContentChanged)
                        .secure(true),
                    text_input("Api URL", &self.api_url)
                        .on_input(LoginPageMessage::APIURLContentChanged),
                ]
                .spacing(10)
                .padding(20),
                column![
                    text_input("Username", &self.username)
                        .on_input(LoginPageMessage::UsernameContentChanged),
                    text_input("Room ID", &self.room_id)
                        .on_input(LoginPageMessage::RoomIDContentChanged),
                ]
                .spacing(10)
                .padding(20),
            ]
            .spacing(10),
            container(button("Connect").on_press(LoginPageMessage::ConnectButtonPressed))
                .padding(20)
                .height(Length::Fill)
                .width(Length::Fill)
                .align_right(Length::Fill),
        ]
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
                    Ok(token) => LoginPageAction::Connect {
                        token,
                        api_url: auth_data.api_url,
                    },
                    Err(error) => LoginPageAction::Failed(error),
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
    AccessToken::with_api_key(&api_key, &api_secret)
        .with_identity(&identity)
        .with_name(&identity)
        .with_grants(VideoGrants {
            room_join: true,
            room: room.clone(),
            ..Default::default()
        })
        .to_jwt()
        .map_err(|e| e.to_string())
}
