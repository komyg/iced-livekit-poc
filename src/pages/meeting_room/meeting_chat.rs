use iced::widget::{button, column, container, rich_text, row, scrollable, span, svg, text_input};
use iced::{Background, Center, Color, Element, Font, Length, border, font, never};

const PANEL_WIDTH: u32 = 320;

#[derive(Debug, Clone)]
pub struct ChatEntry {
    pub sender: String,
    pub body: String,
}

#[derive(Debug, Default)]
pub struct MeetingChat {
    messages: Vec<ChatEntry>,
    draft: String,
}

#[derive(Debug, Clone)]
pub enum MeetingChatMessage {
    DraftChanged(String),
    Send,
}

#[derive(Debug, Clone)]
pub enum MeetingChatAction {
    None,
    Send(String),
}

impl MeetingChat {
    pub const fn new() -> Self {
        Self {
            messages: Vec::new(),
            draft: String::new(),
        }
    }

    pub fn view(&self) -> Element<'_, MeetingChatMessage> {
        let can_send = !self.draft.trim().is_empty();

        let messages = scrollable(
            column(self.messages.iter().map(entry_view))
                .spacing(8)
                .width(Length::Fill),
        )
        .height(Length::Fill)
        .anchor_bottom();

        let send_button_icon = svg::Handle::from_memory(
            include_bytes!("../../../assets/paper-plane-solid-full.svg").as_slice(),
        );
        let send_button_opacity: f32 = if can_send { 1.0 } else { 0.3 };
        let send_button = button(
            svg(send_button_icon)
                .width(20)
                .height(20)
                .opacity(send_button_opacity),
        )
        .padding(10)
        .style(|_, status| {
            let background = Color::from_rgb(1.0, 1.0, 1.0);

            button::Style {
                border: border::rounded(999),
                background: Some(Background::Color(match status {
                    button::Status::Disabled => background.scale_alpha(0.5),
                    _ => background,
                })),
                ..Default::default()
            }
        })
        .on_press_maybe(can_send.then_some(MeetingChatMessage::Send));

        let message_composer = row![
            text_input("Message", &self.draft)
                .on_input(MeetingChatMessage::DraftChanged)
                .on_submit_maybe(can_send.then_some(MeetingChatMessage::Send))
                .width(Length::Fill),
            send_button,
        ]
        .spacing(8)
        .align_y(Center);

        container(column![messages, message_composer].spacing(12))
            .width(PANEL_WIDTH)
            .height(Length::Fill)
            .padding(12)
            .style(container::bordered_box)
            .into()
    }

    pub fn update(&mut self, message: MeetingChatMessage) -> MeetingChatAction {
        match message {
            MeetingChatMessage::DraftChanged(draft) => self.draft = draft,
            MeetingChatMessage::Send => {
                let message = self.draft.trim();
                if !message.is_empty() {
                    let message = message.to_owned();
                    self.draft.clear();

                    return MeetingChatAction::Send(message);
                }
            }
        }

        MeetingChatAction::None
    }

    pub fn push(&mut self, entry: ChatEntry) {
        self.messages.push(entry);
    }
}

fn entry_view(entry: &ChatEntry) -> Element<'_, MeetingChatMessage> {
    rich_text![
        span(format!("{}: ", entry.sender)).font(Font {
            weight: font::Weight::Bold,
            ..Font::DEFAULT
        }),
        span(entry.body.as_str()),
    ]
    // Pins the otherwise uninferable `Link` type; no span carries a link.
    .on_link_click(never)
    .into()
}
