use iced::widget::{button, container, rich_text, span};
use iced::{Background, Color, Element, Font, Task, border, font, never};
use std::time::Duration;

use super::meeting_chat::ChatEntry;

const DISMISS_TIMEOUT: Duration = Duration::from_secs(5);
const TOAST_WIDTH: u32 = 320;

#[derive(Debug, Default)]
pub struct ChatNotification {
    showing: Option<(u64, ChatEntry)>,
    next_id: u64,
}

#[derive(Debug, Clone)]
pub enum ChatNotificationMessage {
    Show(ChatEntry),
    Pressed,
    Dismiss(u64),
}

pub enum ChatNotificationAction {
    None,
    OpenChat,
    /// Work the caller has to run for the toast to behave — currently only the
    /// countdown that takes it back off screen.
    Run(Task<ChatNotificationMessage>),
}

impl ChatNotification {
    pub const fn new() -> Self {
        Self {
            showing: None,
            next_id: 0,
        }
    }

    pub fn view(&self) -> Option<Element<'_, ChatNotificationMessage>> {
        let (_, entry) = self.showing.as_ref()?;

        let preview = rich_text![
            span(format!("{}: ", entry.sender)).font(Font {
                weight: font::Weight::Bold,
                ..Font::DEFAULT
            }),
            span(entry.body.as_str()),
        ]
        .on_link_click(never);

        Some(
            button(container(preview).width(TOAST_WIDTH))
                .padding(12)
                .style(|_, _| button::Style {
                    border: border::rounded(8),
                    background: Some(Background::Color(Color::from_rgb(1.0, 1.0, 1.0))),
                    text_color: Color::BLACK,
                    ..Default::default()
                })
                .on_press(ChatNotificationMessage::Pressed)
                .into(),
        )
    }

    pub fn update(&mut self, message: ChatNotificationMessage) -> ChatNotificationAction {
        match message {
            ChatNotificationMessage::Show(entry) => {
                let id = self.next_id;

                self.showing = Some((id, entry));
                self.next_id = id.wrapping_add(1);

                return ChatNotificationAction::Run(Task::future(async move {
                    tokio::time::sleep(DISMISS_TIMEOUT).await;

                    ChatNotificationMessage::Dismiss(id)
                }));
            }
            ChatNotificationMessage::Pressed => return ChatNotificationAction::OpenChat,
            ChatNotificationMessage::Dismiss(id) => {
                if self.showing.as_ref().is_some_and(|(shown, _)| *shown == id) {
                    self.showing = None;
                }
            }
        }

        ChatNotificationAction::None
    }

    pub fn dismiss(&mut self) {
        self.showing = None;
    }
}
