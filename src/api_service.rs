use livekit_api::access_token::{AccessToken, VideoGrants};

use crate::common::ApiKey;

pub fn get_access_token(auth_data: &ApiKey) -> Result<String, String> {
    let ApiKey {
        api_key,
        api_secret,
        api_url: _,
        identity,
        room,
    } = auth_data;
    AccessToken::with_api_key(api_key, api_secret)
        .with_identity(identity)
        .with_name(identity)
        .with_grants(VideoGrants {
            room_join: true,
            room: room.clone(),
            ..Default::default()
        })
        .to_jwt()
        .map_err(|e| e.to_string())
}
