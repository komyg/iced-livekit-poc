use iced::{
    Element, Length,
    widget::{button, column, container, row, text_input},
};

#[derive(Debug, Default)]
pub struct ConnectorPage {
    api_key: String,
    api_secret: String,
    api_url: String,
    username: String,
    room_id: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    APIKeyContentChanged(String),
    APISecretContentChanged(String),
    APIURLContentChanged(String),
    UsernameContentChanged(String),
    RoomIDContentChanged(String),
    ConnectButtonPressed,
}

impl ConnectorPage {
    pub fn view(&self) -> Element<'_, Message> {
        column![
            row![
                column![
                    text_input("Api Key", &self.api_key).on_input(Message::APIKeyContentChanged),
                    text_input("Api Secret", &self.api_secret)
                        .on_input(Message::APISecretContentChanged)
                        .secure(true),
                    text_input("Api URL", &self.api_url).on_input(Message::APIURLContentChanged),
                ]
                .spacing(10)
                .padding(20),
                column![
                    text_input("Username", &self.username)
                        .on_input(Message::UsernameContentChanged),
                    text_input("Room ID", &self.room_id).on_input(Message::RoomIDContentChanged),
                ]
                .spacing(10)
                .padding(20),
            ]
            .spacing(10),
            container(button("Connect").on_press(Message::ConnectButtonPressed))
                .padding(20)
                .height(Length::Fill)
                .width(Length::Fill)
                .align_right(Length::Fill),
        ]
        .into()
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::APIKeyContentChanged(value) => self.api_key = value,
            Message::APISecretContentChanged(value) => self.api_secret = value,
            Message::APIURLContentChanged(value) => self.api_url = value,
            Message::UsernameContentChanged(value) => self.username = value,
            Message::RoomIDContentChanged(value) => self.room_id = value,
            Message::ConnectButtonPressed => {
                println!("Connect button pressed");
            }
        }
    }
}
