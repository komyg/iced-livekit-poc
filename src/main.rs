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
            LoginPageAction::Connect(api_key) => Task::perform(
                async move {
                    let token = api_service::get_access_token(&api_key)?;
                    api_service::connect_to_room(token, api_key.api_url).await
                },
                Message::Connected,
            ),
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
