use std::sync::Arc;

use iced::futures::stream::BoxStream;
use iced::futures::{
    FutureExt, SinkExt, Stream, StreamExt, select, stream, stream::FuturesUnordered,
    stream::SelectAll,
};
use iced::widget::{container, shader, stack, text};
use iced::{Element, Length, Subscription, padding};
use livekit::id::TrackSid;
use livekit::options::TrackPublishOptions;
use livekit::publication::LocalTrackPublication;
use livekit::track::RemoteTrack;
use livekit::track::TrackSource;
use livekit::track::{LocalAudioTrack, LocalTrack, LocalVideoTrack};
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::AudioSourceOptions;
use livekit::webrtc::audio_source::RtcAudioSource;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use livekit::webrtc::video_frame::{I420Buffer, VideoBuffer, VideoFrame, VideoRotation};
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_source::{RtcVideoSource, VideoResolution};
use livekit::webrtc::video_stream::native::NativeVideoStream;
use livekit::{Room, RoomEvent, RoomOptions};
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use super::meeting_controls::{MeetingControls, MeetingControlsMessage};
use crate::audio::audio_sink::AudioSink;
use crate::audio::audio_source::AudioSource;
use crate::video::video_sink::{Frame, VideoSink, to_i420};
use crate::video::video_source::VideoSource;

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
    LocalFrame(Frame),
    RemoteVideoEnded,
    MeetingControls(MeetingControlsMessage),
    MicrophonePublished(LocalTrackPublication),
    CameraControl(watch::Sender<bool>),
}

pub struct MeetingRoomPage {
    token: String,
    url: String,
    status: Status,
    frame: Option<Frame>,
    local_frame: Option<Frame>,
    meeting_controls: MeetingControls,
    microphone: Option<LocalTrackPublication>,
    camera_control: Option<watch::Sender<bool>>,
}

impl MeetingRoomPage {
    pub const fn new(token: String, url: String) -> Self {
        Self {
            token,
            url,
            status: Status::Connecting,
            frame: None,
            local_frame: None,
            meeting_controls: MeetingControls::new(),
            microphone: None,
            camera_control: None,
        }
    }

    pub fn view(&self) -> Element<'_, MeetingRoomMessage> {
        let displayed = self.frame.as_ref().or(self.local_frame.as_ref());

        let content: Element<'_, MeetingRoomMessage> = match displayed {
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

        stack![
            container(content)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
            container(
                self.meeting_controls
                    .view()
                    .map(MeetingRoomMessage::MeetingControls)
            )
            .center_x(Length::Fill)
            .align_bottom(Length::Fill)
            .padding(padding::bottom(40))
        ]
        .into()
    }

    pub fn update(&mut self, message: MeetingRoomMessage) {
        match message {
            MeetingRoomMessage::Status(status) => self.status = status,
            MeetingRoomMessage::Frame(frame) => self.frame = Some(frame),
            MeetingRoomMessage::RemoteVideoEnded => self.frame = None,
            MeetingRoomMessage::LocalFrame(frame) => {
                if !self.meeting_controls.camera_off {
                    self.local_frame = Some(frame);
                }
            }
            MeetingRoomMessage::MeetingControls(message) => {
                self.meeting_controls.update(message);
                self.apply_controls();

                if self.meeting_controls.camera_off {
                    self.local_frame = None;
                }
            }
            MeetingRoomMessage::MicrophonePublished(publication) => {
                self.microphone = Some(publication);
                self.apply_controls();
            }
            MeetingRoomMessage::CameraControl(sender) => {
                self.camera_control = Some(sender);
                self.apply_controls();
            }
        }
    }

    fn apply_controls(&self) {
        if let Some(microphone) = &self.microphone {
            if self.meeting_controls.microphone_muted {
                microphone.mute();
            } else {
                microphone.unmute();
            }
        }

        if let Some(camera) = &self.camera_control {
            let _ = camera.send(!self.meeting_controls.camera_off);
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

/// Publishes a microphone track matched to the capture device's own format, so
/// WebRTC never has to resample. Returns the source paired with the scratch
/// buffer the capture loop drains into, plus the publication that mute goes
/// through.
async fn publish_microphone(
    room: &Room,
    sample_rate: u32,
    channels: u16,
    frame_len: usize,
) -> Option<(NativeAudioSource, Vec<i16>, LocalTrackPublication)> {
    let rtc = NativeAudioSource::new(
        AudioSourceOptions {
            echo_cancellation: true,
            noise_suppression: true,
            auto_gain_control: true,
        },
        sample_rate,
        u32::from(channels),
        0,
    );
    let track = LocalAudioTrack::create_audio_track("mic", RtcAudioSource::Native(rtc.clone()));

    room.local_participant()
        .publish_track(
            LocalTrack::Audio(track),
            TrackPublishOptions {
                source: TrackSource::Microphone,
                ..Default::default()
            },
        )
        .await
        .map(|publication| (rtc, Vec::with_capacity(frame_len), publication))
        .inspect_err(|error| eprintln!("Failed to publish microphone: {error}"))
        .ok()
}

async fn publish_camera(
    room: &Room,
    width: u32,
    height: u32,
) -> Option<(NativeVideoSource, LocalTrackPublication)> {
    let rtc = NativeVideoSource::new(VideoResolution { width, height }, false);
    let track = LocalVideoTrack::create_video_track("camera", RtcVideoSource::Native(rtc.clone()));

    room.local_participant()
        .publish_track(
            LocalTrack::Video(track),
            TrackPublishOptions {
                source: TrackSource::Camera,
                ..Default::default()
            },
        )
        .await
        .map(|publication| (rtc, publication))
        .inspect_err(|error| eprintln!("Failed to publish camera: {error}"))
        .ok()
}

/// Turns the camera's latest-frame slot into a stream, so it can sit in the
/// same `SelectAll` machinery as the remote tracks.
///
/// Ends when the capture thread drops its sender, which is what makes the
/// `SelectAll` branch go quiet instead of spinning on a dead channel.
fn camera_stream(
    receiver: watch::Receiver<Option<Arc<I420Buffer>>>,
) -> impl Stream<Item = Arc<I420Buffer>> + Send {
    stream::unfold(receiver, async |mut receiver| {
        loop {
            receiver.changed().await.ok()?;

            // Scoped so the borrow guard is released before the next await.
            let frame = receiver.borrow_and_update().clone();

            if let Some(frame) = frame {
                return Some((frame, receiver));
            }
        }
    })
}

fn camera_toggles(receiver: watch::Receiver<bool>) -> impl Stream<Item = bool> + Send {
    stream::unfold(receiver, async |mut receiver| {
        receiver.changed().await.ok()?;

        let camera_wanted = *receiver.borrow_and_update();

        Some((camera_wanted, receiver))
    })
}

fn connect(data: &(String, String)) -> impl Stream<Item = MeetingRoomMessage> + use<> {
    let (url, token) = data.clone();

    // A small buffer plus LiveKit's own keep-newest frame queue means a slow UI
    // drops stale frames at the source instead of accumulating latency. Two
    // producers now share it — the camera and the remote track — so it needs a
    // slot each, or `try_send` starves whichever one loses the race.
    iced::stream::channel(4, async move |mut output| {
        let connection = Room::connect(&url, &token, RoomOptions::default()).await;

        let (room, mut events) = match connection {
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

        let mut videos: SelectAll<NativeVideoStream> = SelectAll::new();
        let mut audio: SelectAll<NativeAudioStream> = SelectAll::new();
        let mut frame_id: u64 = 0;

        let mut remote_video: Option<TrackSid> = None;

        // Diagnostics: which of the two video directions is actually alive.
        let mut remote_frames: u64 = 0;
        let mut local_frames: u64 = 0;

        let mut audio_sink = AudioSink::new()
            .inspect_err(|error| eprintln!("Audio output disabled: {error}"))
            .ok();

        let mut audio_source = AudioSource::new()
            .inspect_err(|error| eprintln!("Audio input disabled: {error}"))
            .ok();

        let mut mic: Option<(NativeAudioSource, Vec<i16>)> = None;
        let format = audio_source
            .as_ref()
            .map(|source| (source.sample_rate, source.channels, source.frame_len()));

        if let Some((sample_rate, channels, frame_len)) = format
            && let Some((rtc, buffer, publication)) =
                publish_microphone(&room, sample_rate, channels, frame_len).await
        {
            let _ = output
                .send(MeetingRoomMessage::MicrophonePublished(publication))
                .await;

            mic = Some((rtc, buffer));
        }

        let (camera_tx, camera_rx) = watch::channel(true);
        let mut camera_toggles = camera_toggles(camera_rx).boxed().fuse();

        let _ = output
            .send(MeetingRoomMessage::CameraControl(camera_tx))
            .await;

        // Don't block while waiting for the camera to open.
        let mut camera_source = match tokio::task::spawn_blocking(VideoSource::new).await {
            Ok(Ok(source)) => Some(source),
            Ok(Err(error)) => {
                eprintln!("Camera disabled: {error}");
                None
            }
            Err(error) => {
                eprintln!("Camera thread panicked: {error}");
                None
            }
        };

        let mut camera_frames: SelectAll<BoxStream<'static, Arc<I420Buffer>>> = SelectAll::new();
        let mut camera: Option<NativeVideoSource> = None;
        let mut camera_publication: Option<LocalTrackPublication> = None;

        let mut camera_opens: FuturesUnordered<
            tokio::task::JoinHandle<Result<VideoSource, String>>,
        > = FuturesUnordered::new();
        let mut camera_on = true;

        if let Some(source) = &camera_source {
            if let Some((rtc, publication)) =
                publish_camera(&room, source.width, source.height).await
            {
                camera = Some(rtc);
                camera_publication = Some(publication);
            }

            camera_frames.push(camera_stream(source.frames()).boxed());
        }

        // Cancel-safe, and cheap: 10 ms is exactly one WebRTC frame.
        let mut ticker = tokio::time::interval(Duration::from_millis(10));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            select! {
                event = events.recv().fuse() => {
                    let Some(event) = event else { break };

                    match event {
                        RoomEvent::TrackSubscribed {
                            track: RemoteTrack::Video(track),
                            ..
                        } if videos.is_empty() => {
                            remote_video = Some(track.sid());
                            videos.push(
                                NativeVideoStream::new(track.rtc_track()),
                            );
                        }
                        RoomEvent::TrackUnsubscribed {
                            track: RemoteTrack::Video(track),
                            ..
                        } if remote_video.as_ref() == Some(&track.sid()) => {
                            // The peer switched its camera off.
                            videos.clear();
                            remote_video = None;
                            remote_frames = 0;

                            let _ = output
                                .send(MeetingRoomMessage::RemoteVideoEnded)
                                .await;
                        }
                        RoomEvent::TrackSubscribed {
                            track: RemoteTrack::Audio(track),
                            ..
                        } if audio.is_empty() => {
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
                        RoomEvent::TrackSubscriptionFailed { error, track_sid, .. } => {
                            eprintln!("Track subscription failed for {track_sid}: {error}");
                        }
                        event => eprintln!("Room event: {event:?}"),
                    }
                }
                frame = videos.next() => {
                    let Some(frame) = frame else { continue };

                    let Some(buffer) = to_i420(&*frame.buffer) else {
                        eprintln!(
                            "Dropped a remote frame in an unconvertible format: {:?}",
                            frame.buffer.buffer_type(),
                        );
                        continue;
                    };

                    remote_frames = remote_frames.wrapping_add(1);
                    if remote_frames == 1 {
                        eprintln!(
                            "First remote frame: {}x{}",
                            buffer.width(),
                            buffer.height(),
                        );
                    }

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
                buffer = camera_frames.next() => {
                    let Some(buffer) = buffer else { continue };

                    // Unlike the mic, video capture is synchronous — every
                    // conversion already happened on the capture thread.
                    if let Some(rtc) = &camera {
                        rtc.capture_frame(&VideoFrame::new(
                            VideoRotation::VideoRotation0,
                            &*buffer,
                        ));

                        local_frames = local_frames.wrapping_add(1);
                        if local_frames == 1 {
                            eprintln!(
                                "First frame captured to the room: {}x{}",
                                buffer.width(),
                                buffer.height(),
                            );
                        }
                    }

                    if remote_frames == 0 {
                        frame_id = frame_id.wrapping_add(1);

                        let _ = output.try_send(MeetingRoomMessage::LocalFrame(Frame::new(
                            buffer,
                            frame_id,
                            VideoRotation::VideoRotation0,
                        )));
                    }
                }
                camera_wanted = camera_toggles.next() => {
                    let Some(camera_wanted) = camera_wanted else { continue };

                    // `watch::send` notifies even when the value is unchanged,
                    // and the page re-sends on every control press — so only a
                    // real edge is worth acting on.
                    if camera_wanted == camera_on {
                        continue;
                    }

                    camera_on = camera_wanted;

                    if camera_wanted {
                        // An open already in flight covers this request; a
                        // second one would fight it for the device.
                        if camera_source.is_none() && camera_opens.is_empty() {
                            camera_opens.push(
                                tokio::task::spawn_blocking(VideoSource::new),
                            );
                        }
                    } else if let Some(source) = camera_source.take() {
                        // Only ever once per publication: livekit unwraps the
                        // track inside `unpublish_track`, so a second call for
                        // the same sid panics.
                        if let Some(publication) = camera_publication.take()
                            && let Err(error) = room
                                .local_participant()
                                .unpublish_track(&publication.sid())
                                .await
                        {
                            eprintln!("Failed to unpublish camera: {error}");
                        }

                        // Stops feeding a track that no longer exists, and
                        // drops the capture stream before the thread winds
                        // down, so no late frame reaches the next camera.
                        camera = None;
                        camera_frames.clear();

                        // `Drop` joins the capture thread, which can be sitting
                        // in a blocking frame grab.
                        tokio::task::spawn_blocking(move || drop(source));
                    }
                }
                opened = camera_opens.next() => {
                    let source = match opened {
                        Some(Ok(Ok(source))) => source,
                        Some(Ok(Err(error))) => {
                            eprintln!("Camera disabled: {error}");
                            continue;
                        }
                        Some(Err(error)) => {
                            eprintln!("Camera thread panicked: {error}");
                            continue;
                        }
                        None => continue,
                    };

                    // Seconds pass while the device comes up, which is long
                    // enough for the switch to have flipped back.
                    if !camera_on {
                        tokio::task::spawn_blocking(move || drop(source));
                        continue;
                    }

                    if let Some((rtc, publication)) =
                        publish_camera(&room, source.width, source.height).await
                    {
                        camera = Some(rtc);
                        camera_publication = Some(publication);
                    }

                    camera_frames.clear();
                    camera_frames.push(camera_stream(source.frames()).boxed());
                    camera_source = Some(source);

                    // So the first-frame log below reports the restart too.
                    local_frames = 0;
                }
                _ = ticker.tick().fuse() => {
                    let (Some(source), Some((rtc, buffer))) = (&mut audio_source, &mut mic)
                        else { continue };

                    // Drain whatever the mic has queued; false = less than a full
                    // frame ready, so we wait for the next tick.
                    //
                    // Muting is handled by the page disabling the RTC track, so
                    // this loop keeps feeding it either way — which also keeps
                    // the ring empty. Stalling here would not: it holds 200 ms
                    // and cpal drops new samples once full, so unmuting would
                    // replay stale audio.
                    while source.pop_frame(buffer) {
                        let frame = AudioFrame {
                            data: buffer.as_slice().into(),
                            sample_rate: source.sample_rate,
                            num_channels: u32::from(source.channels),
                            samples_per_channel: source.sample_rate / 100,
                        };

                        if let Err(error) = rtc.capture_frame(&frame).await {
                            eprintln!("Mic capture failed: {error}");
                            break;
                        }
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
