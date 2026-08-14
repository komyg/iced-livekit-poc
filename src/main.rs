mod common;
mod pages;
mod video;

use iced::{Element, Subscription};
use pages::login::LoginPage;
use pages::meeting_room::MeetingRoomPage;

use crate::pages::login::{LoginPageAction, LoginPageMessage};
use crate::pages::meeting_room::MeetingRoomMessage;

enum Screen {
    Login(LoginPage),
    MeetingRoom(MeetingRoomPage),
}

impl Screen {
    fn new() -> Self {
        Screen::Login(LoginPage::default())
    }
}

enum Message {
    Login(LoginPageMessage),
    MeetingRoom(MeetingRoomMessage),
}

fn update(screen: &mut Screen, message: Message) {
    match message {
        Message::Login(message) => {
            let Screen::Login(page) = screen else {
                return;
            };

            match page.update(message) {
                LoginPageAction::Login { token, api_url } => {
                    // The meeting room's subscription drives the connection, so
                    // switching the screen is all it takes to start it.
                    *screen =
                        Screen::MeetingRoom(MeetingRoomPage::new(token, api_url));
                }
                LoginPageAction::None => {}
            }
        }
        Message::MeetingRoom(message) => {
            let Screen::MeetingRoom(page) = screen else {
                return;
            };

            page.update(message);
        }
    }
}

fn view(screen: &Screen) -> Element<'_, Message> {
    match screen {
        Screen::Login(page) => page.view().map(Message::Login),
        Screen::MeetingRoom(page) => page.view().map(Message::MeetingRoom),
    }
}

fn subscription(screen: &Screen) -> Subscription<Message> {
    match screen {
        Screen::Login(_) => Subscription::none(),
        Screen::MeetingRoom(page) => {
            page.subscription().map(Message::MeetingRoom)
        }
    }
}

pub fn main() -> iced::Result {
    // Without this the shader path fails silently: if wgpu is unavailable iced
    // falls back to tiny-skia, which only `log::warn!`s that it cannot draw
    // custom primitives and then renders nothing. Also surfaces LiveKit's
    // dropped-frame warnings and iced's adapter/surface-format selection.
    env_logger::init();

    iced::application(Screen::new, update, view)
        .subscription(subscription)
        .title("PV Meet Connector")
        .run()
}
