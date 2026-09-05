use iced::widget::{
    button, column, combo_box, container, rich_text, row, scrollable, span, svg, text_input,
};
use iced::{Background, Center, Color, Element, Font, Length, border, font, never};
use rust_i18n::t;
use std::collections::HashMap;

use super::data::{EVERYONE_ID, Recipient, Roster};

const PANEL_WIDTH: u32 = 320;

#[derive(Debug, Clone)]
pub struct ChatEntry {
    pub sender: String,
    pub recipient: Recipient,
    pub body: String,
}

impl ChatEntry {
    pub fn header(&self) -> String {
        t!(
            "chat.entry_header",
            sender = self.sender,
            recipient = self.recipient.label
        )
        .into_owned()
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub body: String,
    pub recipient: Recipient,
}

#[derive(Debug)]
pub struct MeetingChat {
    messages: Vec<ChatEntry>,
    draft: String,
    recipients: combo_box::State<Recipient>,
    recipient: Recipient,
}

#[derive(Debug, Clone)]
pub enum MeetingChatMessage {
    DraftChanged(String),
    RecipientSelected(Recipient),
    ParticipantsChanged(Roster),
    Send,
}

#[derive(Debug, Clone)]
pub enum MeetingChatAction {
    None,
    Send(ChatRequest),
}

impl MeetingChat {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            draft: String::new(),
            recipients: combo_box::State::new(vec![Recipient::everyone()]),
            recipient: Recipient::everyone(),
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

        let recipient_picker = combo_box(
            &self.recipients,
            &t!("chat.everyone"),
            Some(&self.recipient),
            MeetingChatMessage::RecipientSelected,
        )
        .width(Length::Fill);

        let message_composer = column![
            recipient_picker,
            row![
                text_input(&t!("chat.message"), &self.draft)
                    .on_input(MeetingChatMessage::DraftChanged)
                    .on_submit_maybe(can_send.then_some(MeetingChatMessage::Send))
                    .width(Length::Fill),
                send_button,
            ]
            .spacing(8)
            .align_y(Center),
        ]
        .spacing(8);

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
            MeetingChatMessage::RecipientSelected(recipient) => self.recipient = recipient,
            MeetingChatMessage::ParticipantsChanged(roster) => {
                let options = recipients_of(&roster);
                if self.recipients.options() != options.as_slice() {
                    // Rather than quietly redirect the next message to whoever
                    // sorted into the same slot, fall back to the room when the
                    // pick has left.
                    if !options
                        .iter()
                        .any(|recipient| recipient.id == self.recipient.id)
                    {
                        self.recipient = Recipient::everyone();
                    }

                    self.recipients = combo_box::State::new(options);
                }
            }
            MeetingChatMessage::Send => {
                let message = self.draft.trim();
                if !message.is_empty() {
                    let body = message.to_owned();
                    self.draft.clear();

                    return MeetingChatAction::Send(ChatRequest {
                        body,
                        recipient: self.recipient.clone(),
                    });
                }
            }
        }

        MeetingChatAction::None
    }

    pub fn push(&mut self, entry: ChatEntry) {
        self.messages.push(entry);
    }
}

fn recipients_of(roster: &Roster) -> Vec<Recipient> {
    let mut duplicates: HashMap<&str, usize> = HashMap::new();
    for member in roster.remotes() {
        *duplicates.entry(member.label.as_str()).or_default() += 1;
    }

    // Remotes only: we cannot message ourselves.
    let mut recipients: Vec<Recipient> = roster
        .remotes()
        .filter(|member| member.identity != EVERYONE_ID)
        .map(|member| Recipient {
            id: member.identity.clone(),
            label: if duplicates
                .get(member.label.as_str())
                .is_some_and(|count| *count > 1)
            {
                format!("{} ({})", member.label, member.identity)
            } else {
                member.label.clone()
            },
        })
        .collect();

    recipients.sort_by_key(|recipient| recipient.label.to_lowercase());
    recipients.insert(0, Recipient::everyone());

    recipients
}

fn entry_view(entry: &ChatEntry) -> Element<'_, MeetingChatMessage> {
    rich_text![
        span(entry.header()).font(Font {
            weight: font::Weight::Bold,
            ..Font::DEFAULT
        }),
        span(entry.body.as_str()),
    ]
    // Pins the otherwise uninferable `Link` type; no span carries a link.
    .on_link_click(never)
    .into()
}
