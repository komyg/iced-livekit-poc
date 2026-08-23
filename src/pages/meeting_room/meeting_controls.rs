use iced::widget::{button, svg};
use iced::{Background, Color, Element, border};

#[derive(Default, Debug, Copy, Clone)]
pub(super) struct MeetingControls {
    pub(super) microphone_muted: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum MeetingControlsMessage {
    ToggleMicrophone,
}

impl MeetingControls {
    pub(super) const fn new() -> Self {
        Self {
            microphone_muted: false,
        }
    }

    pub(super) fn view(&self) -> Element<'_, MeetingControlsMessage> {
        let microphone_icon = match self.microphone_muted {
            false => svg::Handle::from_memory(
                include_bytes!("../../../assets/microphone-solid-full.svg").as_slice(),
            ),
            true => svg::Handle::from_memory(
                include_bytes!("../../../assets/microphone-slash-solid-full.svg").as_slice(),
            ),
        };

        button(svg(microphone_icon).width(24).height(24))
            .padding(12)
            .style(|_, _| button::Style {
                border: border::rounded(999),
                background: Some(Background::Color(Color::from_rgb(1.0, 1.0, 1.0))),
                ..Default::default()
            })
            .on_press(MeetingControlsMessage::ToggleMicrophone)
            .into()
    }

    pub(super) fn update(&mut self, message: MeetingControlsMessage) {
        match message {
            MeetingControlsMessage::ToggleMicrophone => {
                self.microphone_muted = !self.microphone_muted;
            }
        }
    }
}
