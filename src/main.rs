mod common;
mod pages;

use iced::{Element, Task};
use pages::login::LoginPage;
use pages::meeting_room::MeetingRoomPage;

use crate::pages::login::{LoginPageAction, LoginPageMessage};
use crate::pages::meeting_room::{MeetingRoomAction, MeetingRoomMessage};

enum Screen {
    Login(LoginPage),
    MeetingRoom(MeetingRoomPage),
}

impl Screen {
    fn new() -> Self {
        Screen::Login(LoginPage::default())
    }
}

#[derive(Debug, Clone)]
enum Message {
    Login(LoginPageMessage),
    MeetingRoom(MeetingRoomMessage),
}

fn update(screen: &mut Screen, message: Message) -> Task<Message> {
    match message {
        Message::Login(message) => {
            let Screen::Login(page) = screen else {
                return Task::none();
            };

            match page.update(message) {
                LoginPageAction::Login { token, api_url } => {
                    *screen = Screen::MeetingRoom(MeetingRoomPage::default(token, api_url));
                    Task::none()
                }
                LoginPageAction::None => Task::none(),
            }
        }
        Message::MeetingRoom(message) => {
            let Screen::MeetingRoom(page) = screen else {
                return Task::none();
            };

            match page.update(message) {
                MeetingRoomAction::None => Task::none(),
            }
        }
    }
}

fn view(screen: &Screen) -> Element<'_, Message> {
    match screen {
        Screen::Login(page) => page.view().map(Message::Login),
        Screen::MeetingRoom(page) => page.view().map(Message::MeetingRoom),
    }
}

pub fn main() -> iced::Result {
    iced::application(Screen::new, update, view)
        .title("PV Meet Connector")
        .run()
}
