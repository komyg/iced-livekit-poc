mod common;
mod pages;

use iced::{Element, Task};
use pages::login::LoginPage;

use crate::pages::login::{LoginPageAction, LoginPageMessage};

#[derive(Debug, Clone)]
enum Message {
    Login(LoginPageMessage),
}

fn update(page: &mut LoginPage, message: Message) -> Task<Message> {
    match message {
        Message::Login(message) => match page.update(message) {
            LoginPageAction::Login {
                token: _token,
                api_url: _api_url,
            } => Task::none(),
            LoginPageAction::None => Task::none(),
        },
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
