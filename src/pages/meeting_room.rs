use iced::{
    Element,
    widget::{column, text},
};
use livekit::{Room, RoomEvent, RoomOptions};
use tokio::sync::mpsc::UnboundedReceiver;

pub async fn connect_to_room(
    token: String,
    url: String,
) -> Result<(Room, UnboundedReceiver<RoomEvent>), String> {
    let res = Room::connect(&url, &token, RoomOptions::default())
        .await
        .map_err(|e| e.to_string())?;
    return Ok(res);
}

pub struct MeetingRoomPage {
    room: Option<Room>,
    events: Option<UnboundedReceiver<RoomEvent>>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum MeetingRoomMessage {
    None,
}

pub enum MeetingRoomAction {
    None,
}

impl MeetingRoomPage {
    pub fn default(token: String, url: String) -> Self {
        Self {
            room: None,
            events: None,
            error: None,
        }
    }

    pub fn view(&self) -> Element<'_, MeetingRoomMessage> {
        column![text("Meeting Room"),].into()
    }

    pub fn update(&mut self, message: MeetingRoomMessage) -> MeetingRoomAction {
        match message {
            MeetingRoomMessage::None => MeetingRoomAction::None,
        }
    }
}
