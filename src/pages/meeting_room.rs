use std::sync::Arc;

use iced::futures::{FutureExt, SinkExt, Stream, StreamExt, select, stream::SelectAll};
use iced::widget::{container, shader, text};
use iced::{Element, Length, Subscription};
use livekit::track::RemoteTrack;
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use livekit::webrtc::video_stream::native::NativeVideoStream;
use livekit::{Room, RoomEvent, RoomOptions};

use crate::audio::AudioSink;
use crate::video::{Frame, VideoSink, to_i420};

#[derive(Debug, Clone)]
pub enum Status {
    Connecting,
    Live,
    Reconnecting,
    Ended(Option<String>),
}

impl Status {
    fn label(&self) -> String {
        match self {
            Self::Connecting => "Connecting…".to_owned(),
            Self::Live => "Waiting for someone to publish video…".to_owned(),
            Self::Reconnecting => "Reconnecting…".to_owned(),
            Self::Ended(None) => "Disconnected.".to_owned(),
            Self::Ended(Some(error)) => error.clone(),
        }
    }

    const fn is_error(&self) -> bool {
        matches!(self, Self::Ended(Some(_)))
    }
}

#[derive(Debug, Clone)]
pub enum MeetingRoomMessage {
    Status(Status),
    Frame(Frame),
}

pub struct MeetingRoomPage {
    token: String,
    url: String,
    status: Status,
    frame: Option<Frame>,
}

impl MeetingRoomPage {
    pub const fn new(token: String, url: String) -> Self {
        Self {
            token,
            url,
            status: Status::Connecting,
            frame: None,
        }
    }

    pub fn view(&self) -> Element<'_, MeetingRoomMessage> {
        let content: Element<'_, MeetingRoomMessage> = match &self.frame {
            Some(frame) if !self.status.is_error() => shader(VideoSink::new(frame.clone()))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            _ => {
                let label = text(self.status.label());

                if self.status.is_error() {
                    label.style(text::danger).into()
                } else {
                    label.into()
                }
            }
        };

        container(content)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    pub fn update(&mut self, message: MeetingRoomMessage) {
        match message {
            MeetingRoomMessage::Status(status) => self.status = status,
            MeetingRoomMessage::Frame(frame) => self.frame = Some(frame),
        }
    }

    /// Owns the room connection for as long as this page is on screen.
    ///
    /// Keeping the `Room` inside the stream rather than in page state gives it
    /// exactly the right lifetime — dropping the subscription tears down the
    /// room and every video stream together.
    pub fn subscription(&self) -> Subscription<MeetingRoomMessage> {
        Subscription::run_with((self.url.clone(), self.token.clone()), connect)
    }
}

fn connect(data: &(String, String)) -> impl Stream<Item = MeetingRoomMessage> + use<> {
    let (url, token) = data.clone();

    // A small buffer plus LiveKit's own keep-newest frame queue means a slow UI
    // drops stale frames at the source instead of accumulating latency.
    iced::stream::channel(2, async move |mut output| {
        let connection = Room::connect(&url, &token, RoomOptions::default()).await;

        let (_room, mut events) = match connection {
            Ok(connection) => connection,
            Err(error) => {
                let _ = output
                    .send(MeetingRoomMessage::Status(Status::Ended(Some(
                        error.to_string(),
                    ))))
                    .await;
                return;
            }
        };

        let _ = output.send(MeetingRoomMessage::Status(Status::Live)).await;

        // `SelectAll` drops substreams that finish, and reports itself as
        // terminated while empty — so `select!` skips the branch entirely
        // rather than spinning on a stream that has already returned `None`.
        let mut videos: SelectAll<NativeVideoStream> = SelectAll::new();
        let mut audio: SelectAll<NativeAudioStream> = SelectAll::new();
        let mut frame_id: u64 = 0;

        // The sink lives for the loop's lifetime and stops playback when dropped.
        let mut audio_sink = AudioSink::new()
            .inspect_err(|error| eprintln!("Audio disabled: {error}"))
            .ok();

        loop {
            select! {
                event = events.recv().fuse() => {
                    let Some(event) = event else { break };

                    match event {
                        RoomEvent::TrackSubscribed {
                            track: RemoteTrack::Video(track),
                            ..
                        } if videos.is_empty() => {
                            videos.push(
                                NativeVideoStream::new(track.rtc_track()),
                            );
                        }
                        RoomEvent::TrackSubscribed {
                            track: RemoteTrack::Audio(track),
                            ..
                        } if audio.is_empty() => {
                            // Ask WebRTC to decode straight to the device's own
                            // rate/channels so playback never has to resample.
                            // No sink means no output device — skip decoding.
                            if let Some(sink) = &audio_sink
                                && let Ok(rate) = i32::try_from(sink.sample_rate)
                            {
                                let channels = i32::from(sink.channels);
                                audio.push(NativeAudioStream::new(
                                    track.rtc_track(),
                                    rate,
                                    channels,
                                ));
                            }
                        }
                        RoomEvent::Reconnecting => {
                            let _ = output
                                .send(MeetingRoomMessage::Status(
                                    Status::Reconnecting,
                                ))
                                .await;
                        }
                        RoomEvent::Reconnected => {
                            let _ = output
                                .send(MeetingRoomMessage::Status(Status::Live))
                                .await;
                        }
                        RoomEvent::Disconnected { reason } => {
                            let _ = output
                                .send(MeetingRoomMessage::Status(
                                    Status::Ended(Some(format!("{reason:?}"))),
                                ))
                                .await;
                            break;
                        }
                        _ => {}
                    }
                }
                frame = videos.next() => {
                    let Some(frame) = frame else { continue };

                    let Some(buffer) = to_i420(&*frame.buffer) else {
                        continue;
                    };

                    frame_id = frame_id.wrapping_add(1);

                    let _ = output
                        .try_send(MeetingRoomMessage::Frame(Frame::new(
                            Arc::new(buffer),
                            frame_id,
                            frame.rotation,
                        )));
                }
                frame = audio.next() => {
                    let Some(frame) = frame else { continue };

                    if let Some(sink) = &mut audio_sink {
                        sink.push(&frame);
                    }
                }
                // Required: `select!` panics if every branch is terminated and
                // this arm is missing.
                complete => break,
            }
        }

        let _ = output
            .send(MeetingRoomMessage::Status(Status::Ended(None)))
            .await;
    })
}
