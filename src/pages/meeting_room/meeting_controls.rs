use iced::widget::{Button, button, row, svg};
use iced::{Background, Color, Element, border};

#[derive(Default, Debug, Copy, Clone)]
pub(super) struct MeetingControls {
    pub(super) microphone_muted: bool,
    pub(super) camera_off: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum MeetingControlsMessage {
    ToggleMicrophone,
    ToggleCamera,
}

impl MeetingControls {
    pub(super) const fn new() -> Self {
        Self {
            microphone_muted: false,
            camera_off: false,
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

        let camera_icon = match self.camera_off {
            false => svg::Handle::from_memory(
                include_bytes!("../../../assets/video-solid-full.svg").as_slice(),
            ),
            true => svg::Handle::from_memory(
                include_bytes!("../../../assets/video-slash-solid-full.svg").as_slice(),
            ),
        };

        row![
            control_button(microphone_icon, MeetingControlsMessage::ToggleMicrophone),
            control_button(camera_icon, MeetingControlsMessage::ToggleCamera),
        ]
        .spacing(12)
        .into()
    }

    pub(super) fn update(&mut self, message: MeetingControlsMessage) {
        match message {
            MeetingControlsMessage::ToggleMicrophone => {
                self.microphone_muted = !self.microphone_muted;
            }
            MeetingControlsMessage::ToggleCamera => {
                self.camera_off = !self.camera_off;
            }
        }
    }
}

fn control_button<'a>(
    icon: svg::Handle,
    message: MeetingControlsMessage,
) -> Button<'a, MeetingControlsMessage> {
    button(svg(icon).width(24).height(24))
        .padding(12)
        .style(|_, _| button::Style {
            border: border::rounded(999),
            background: Some(Background::Color(Color::from_rgb(1.0, 1.0, 1.0))),
            ..Default::default()
        })
        .on_press(message)
}
