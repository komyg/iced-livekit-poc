#[macro_use]
extern crate rust_i18n;

mod audio;
mod common;
mod pages;
mod video;

use iced::{Element, Subscription, Task};
use pages::login::LoginPage;
use pages::meeting_room::MeetingRoomPage;

use crate::pages::login::{LoginPageAction, LoginPageMessage};
use crate::pages::meeting_room::MeetingRoomMessage;

i18n!("locales", fallback = "en");

#[expect(
    clippy::large_enum_variant,
    reason = "iced owns the single `Screen` inside its own pinned future, so the \
              meeting room is heap-resident either way; boxing it would only add \
              an allocation and a pointer hop."
)]
enum Screen {
    Login(LoginPage),
    MeetingRoom(MeetingRoomPage),
}

impl Screen {
    fn new() -> Self {
        Self::Login(LoginPage::default())
    }
}

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
                    // The meeting room's subscription drives the connection, so
                    // switching the screen is all it takes to start it.
                    *screen = Screen::MeetingRoom(MeetingRoomPage::new(token, api_url));
                }
                LoginPageAction::None => {}
            }
        }
        Message::MeetingRoom(message) => {
            let Screen::MeetingRoom(page) = screen else {
                return Task::none();
            };

            return page.update(message).map(Message::MeetingRoom);
        }
    }

    Task::none()
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
        Screen::MeetingRoom(page) => page.subscription().map(Message::MeetingRoom),
    }
}

pub fn main() -> iced::Result {
    // Without this the shader path fails silently: if wgpu is unavailable iced
    // falls back to tiny-skia, which only `log::warn!`s that it cannot draw
    // custom primitives and then renders nothing. Also surfaces LiveKit's
    // dropped-frame warnings and iced's adapter/surface-format selection.
    env_logger::init();

    // Before the first screen is built: `Recipient::everyone` and the other
    // labels resolve their text once, at construction.
    common::set_locale_from_env();

    iced::application(Screen::new, update, view)
        .subscription(subscription)
        .title("PV Meet Connector")
        .run()
}
