use iced::{
    Element, Task,
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

// #[derive(Debug, Clone)]
pub enum MeetingRoomMessage {
    Mounted(String, String),
    Connected(Result<(Room, UnboundedReceiver<RoomEvent>), String>),
}

impl MeetingRoomPage {
    pub fn new(token: String, url: String) -> (Self, MeetingRoomMessage) {
        (
            Self {
                room: None,
                events: None,
                error: None,
            },
            MeetingRoomMessage::Mounted(token, url),
        )
    }

    pub fn view(&self) -> Element<'_, MeetingRoomMessage> {
        column![text("Meeting Room"),].into()
    }

    pub fn update(&mut self, message: MeetingRoomMessage) -> Task<MeetingRoomMessage> {
        match message {
            MeetingRoomMessage::Mounted(token, url) => {
                Task::perform(connect_to_room(token, url), MeetingRoomMessage::Connected)
            }
            MeetingRoomMessage::Connected(result) => match result {
                Ok((room, events)) => {
                    self.room = Some(room);
                    self.events = Some(events);
                    Task::none()
                }
                Err(error) => {
                    self.error = Some(error);
                    Task::none()
                }
            },
        }
    }
}
