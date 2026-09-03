//! The non-mosaic stage: one participant, filling the whole area.
//!
//! Owns only what that view needs — the tiles it picks from — so the page
//! keeps the rest of the meeting state to itself.

use iced::widget::shader;
use iced::{Element, Length};

use super::mosaic_view::Tile;
use crate::video::video_sink::{Frame, VideoSink};

/// What the single view shows: a remote with video when there is one,
/// otherwise our own preview. `tiles` is already sorted, so "first remote" is
/// alphabetical.
pub fn featured<'a>(tiles: &[Tile<'a>]) -> Option<(&'a str, &'a Frame)> {
    let with_video = |local: bool| {
        tiles
            .iter()
            .find(|tile| tile.is_local == local && tile.frame.is_some())
    };

    with_video(false)
        .or_else(|| with_video(true))
        .and_then(|tile| Some((tile.identity, tile.frame?)))
}

/// `None` when nobody is publishing video, which is the caller's cue to show
/// its own status text instead.
pub fn view<'a, Message: 'a>(tiles: &[Tile<'a>]) -> Option<Element<'a, Message>> {
    let (identity, frame) = featured(tiles)?;

    Some(
        shader(VideoSink::new(identity, frame.clone()))
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use livekit::webrtc::video_frame::I420Buffer;
    use livekit::webrtc::video_frame::VideoRotation;

    use super::*;

    fn frame() -> Frame {
        Frame::new(
            Arc::new(I420Buffer::new(2, 2)),
            0,
            VideoRotation::VideoRotation0,
        )
    }

    fn tile<'a>(identity: &'a str, is_local: bool, frame: Option<&'a Frame>) -> Tile<'a> {
        Tile {
            identity,
            label: identity,
            frame,
            is_local,
        }
    }

    #[test]
    fn no_video_anywhere_features_nobody() {
        let tiles = [tile("remote", false, None), tile("local", true, None)];

        assert!(featured(&tiles).is_none());
    }

    #[test]
    fn a_remote_with_video_wins_over_our_own_preview() {
        let frame = frame();
        let tiles = [
            tile("local", true, Some(&frame)),
            tile("remote-quiet", false, None),
            tile("remote-live", false, Some(&frame)),
        ];

        assert_eq!(
            featured(&tiles).map(|(identity, _)| identity),
            Some("remote-live")
        );
    }

    #[test]
    fn our_own_preview_is_the_fallback() {
        let frame = frame();
        let tiles = [
            tile("remote", false, None),
            tile("local", true, Some(&frame)),
        ];

        assert_eq!(
            featured(&tiles).map(|(identity, _)| identity),
            Some("local")
        );
    }
}
