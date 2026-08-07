use livekit::{Room, RoomOptions};

pub async fn connect_to_room(token: String, url: String) -> Result<(), String> {
    let res = Room::connect(&url, &token, RoomOptions::default())
        .await
        .map_err(|e| e.to_string())?;
    println!("Connected to room: {:#?}", res);
    Ok(())
}

pub struct MeetingRoom {
    room: Room,
}
