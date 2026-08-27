use iced::widget::{button, column, container, row, scrollable, svg, text, text_input};
use iced::{Background, Center, Color, Element, Length, border};

const PANEL_WIDTH: u32 = 320;

#[derive(Debug, Default)]
pub struct MeetingChat {
    messages: Vec<String>,
    draft: String,
}

#[derive(Debug, Clone)]
pub enum MeetingChatMessage {
    DraftChanged(String),
    Send,
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
            column(self.messages.iter().map(|message| text(message).into()))
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

    pub fn update(&mut self, message: MeetingChatMessage) {
        match message {
            MeetingChatMessage::DraftChanged(draft) => self.draft = draft,
            MeetingChatMessage::Send => {
                let message = self.draft.trim();

                if !message.is_empty() {
                    self.messages.push(message.to_owned());
                    self.draft.clear();
                }
            }
        }
    }
}
