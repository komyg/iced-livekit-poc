mod api_service;
mod common;
mod pages;

use iced::{Element, Task};
use pages::login::LoginPage;

use crate::pages::login::{LoginPageAction, LoginPageMessage};

#[derive(Debug, Clone)]
enum Message {
    Login(LoginPageMessage),
    Connected(Result<(), String>),
}

fn update(page: &mut LoginPage, message: Message) -> Task<Message> {
    match message {
        Message::Login(message) => match page.update(message) {
            LoginPageAction::Connect { token, api_url } => Task::perform(
                async move { api_service::connect_to_room(token, api_url).await },
                Message::Connected,
            ),
            LoginPageAction::Failed(error) => {
                eprintln!("Failed to create access token: {error}");
                Task::none()
            }
            LoginPageAction::None => Task::none(),
        },
        Message::Connected(result) => {
            println!("Connected to room: {result:?}");
            Task::none()
        }
    }
}

fn view(page: &LoginPage) -> Element<'_, Message> {
    page.view().map(Message::Login)
}

pub fn main() -> iced::Result {
    iced::application(LoginPage::default, update, view)
        .title("PV Meet Connector")
        .run()
}
