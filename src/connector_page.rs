use iced::{
    Element, Length,
    widget::{button, column, container, row, text_input},
};

#[derive(Debug, Default)]
pub struct ConnectorPage {
    pub api_key: String,
    pub api_secret: String,
    pub api_url: String,
    pub username: String,
    pub room_id: String,
}

#[derive(Debug, Clone)]
pub enum ConnectorPageMessage {
    APIKeyContentChanged(String),
    APISecretContentChanged(String),
    APIURLContentChanged(String),
    UsernameContentChanged(String),
    RoomIDContentChanged(String),
    ConnectButtonPressed,
}

#[derive(Debug, Clone)]
pub enum ConnectorPageAction {
    Connect,
    None,
}

impl ConnectorPage {
    pub fn view(&self) -> Element<'_, ConnectorPageMessage> {
        column![
            row![
                column![
                    text_input("Api Key", &self.api_key)
                        .on_input(ConnectorPageMessage::APIKeyContentChanged),
                    text_input("Api Secret", &self.api_secret)
                        .on_input(ConnectorPageMessage::APISecretContentChanged)
                        .secure(true),
                    text_input("Api URL", &self.api_url)
                        .on_input(ConnectorPageMessage::APIURLContentChanged),
                ]
                .spacing(10)
                .padding(20),
                column![
                    text_input("Username", &self.username)
                        .on_input(ConnectorPageMessage::UsernameContentChanged),
                    text_input("Room ID", &self.room_id)
                        .on_input(ConnectorPageMessage::RoomIDContentChanged),
                ]
                .spacing(10)
                .padding(20),
            ]
            .spacing(10),
            container(button("Connect").on_press(ConnectorPageMessage::ConnectButtonPressed))
                .padding(20)
                .height(Length::Fill)
                .width(Length::Fill)
                .align_right(Length::Fill),
        ]
        .into()
    }

    pub fn update(&mut self, message: ConnectorPageMessage) -> ConnectorPageAction {
        match message {
            ConnectorPageMessage::APIKeyContentChanged(value) => self.api_key = value,
            ConnectorPageMessage::APISecretContentChanged(value) => self.api_secret = value,
            ConnectorPageMessage::APIURLContentChanged(value) => self.api_url = value,
            ConnectorPageMessage::UsernameContentChanged(value) => self.username = value,
            ConnectorPageMessage::RoomIDContentChanged(value) => self.room_id = value,
            ConnectorPageMessage::ConnectButtonPressed => {
                return ConnectorPageAction::Connect;
            }
        }

        ConnectorPageAction::None
    }
}
