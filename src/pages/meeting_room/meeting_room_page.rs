use std::hash::{Hash, Hasher};
use std::sync::Arc;

use iced::futures::stream::BoxStream;
use iced::futures::{FutureExt, SinkExt, Stream, StreamExt, select, stream, stream::SelectAll};
use iced::widget::{container, shader, stack, text};
use iced::{Element, Length, Subscription, padding};
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
    MeetingControls(MeetingControlsMessage),
}

/// What identifies the room connection, plus the control channel that rides
/// along with it.
///
/// A subscription only ever emits messages upwards, so anything the UI needs to
/// tell the room mid-call has to travel out of band — here, a `watch` slot the
/// page writes and the stream reads.
#[derive(Clone)]
struct Connection {
    url: String,
    token: String,
    controls: watch::Receiver<MeetingControls>,
}

impl Hash for Connection {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Deliberately excludes `controls`: this hash is the subscription's
        // identity, so folding a live control into it would tear the room down
        // and reconnect on every mute toggle.
        self.url.hash(state);
        self.token.hash(state);
    }
}

pub struct MeetingRoomPage {
    token: String,
    url: String,
    status: Status,
    frame: Option<Frame>,
    local_frame: Option<Frame>,
    meeting_controls: MeetingControls,
    controls: watch::Sender<MeetingControls>,
}

impl MeetingRoomPage {
    pub fn new(token: String, url: String) -> Self {
        let meeting_controls = MeetingControls::new();
        Self {
            token,
            url,
            status: Status::Connecting,
            frame: None,
            local_frame: None,
            meeting_controls,
            controls: watch::Sender::new(meeting_controls),
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
            MeetingRoomMessage::LocalFrame(frame) => self.local_frame = Some(frame),
            MeetingRoomMessage::MeetingControls(message) => {
                self.meeting_controls.update(message);

                // `send_modify` rather than `send`: the latter errors while no
                // receiver exists, which is exactly the window between building
                // the page and the subscription starting.
                let controls = self.meeting_controls;
                self.controls.send_modify(|slot| *slot = controls);
            }
        }
    }

    /// Owns the room connection for as long as this page is on screen.
    ///
    /// Keeping the `Room` inside the stream rather than in page state gives it
    /// exactly the right lifetime — dropping the subscription tears down the
    /// room and every video stream together.
    pub fn subscription(&self) -> Subscription<MeetingRoomMessage> {
        Subscription::run_with(
            Connection {
                url: self.url.clone(),
                token: self.token.clone(),
                controls: self.controls.subscribe(),
            },
            connect,
        )
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

/// Publishes a camera track sized to what the device actually gave us.
///
/// `None` means the track never made it to the room, so captured frames are
/// still worth showing locally but must not be handed to WebRTC.
async fn publish_camera(room: &Room, width: u32, height: u32) -> Option<NativeVideoSource> {
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
        .map(|_publication| rtc)
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

fn connect(data: &Connection) -> impl Stream<Item = MeetingRoomMessage> + use<> {
    let Connection {
        url,
        token,
        mut controls,
    } = data.clone();

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

        // `SelectAll` drops substreams that finish, and reports itself as
        // terminated while empty — so `select!` skips the branch entirely
        // rather than spinning on a stream that has already returned `None`.
        let mut videos: SelectAll<NativeVideoStream> = SelectAll::new();
        let mut audio: SelectAll<NativeAudioStream> = SelectAll::new();
        let mut frame_id: u64 = 0;

        // Diagnostics: which of the two video directions is actually alive.
        let mut remote_frames: u64 = 0;
        let mut local_frames: u64 = 0;

        // The sink lives for the loop's lifetime and stops playback when dropped.
        let mut audio_sink = AudioSink::new()
            .inspect_err(|error| eprintln!("Audio output disabled: {error}"))
            .ok();

        let mut audio_source = AudioSource::new()
            .inspect_err(|error| eprintln!("Audio input disabled: {error}"))
            .ok();

        let mut mic: Option<(NativeAudioSource, Vec<i16>, LocalTrackPublication)> = None;
        let format = audio_source
            .as_ref()
            .map(|source| (source.sample_rate, source.channels, source.frame_len()));

        if let Some((sample_rate, channels, frame_len)) = format {
            mic = publish_microphone(&room, sample_rate, channels, frame_len).await;
        }

        // Seeded from the slot rather than assumed false, so a toggle that
        // landed while the room was still connecting is not lost.
        let mut microphone_muted = controls.borrow_and_update().microphone_muted;

        if microphone_muted && let Some((_, _, publication)) = &mic {
            publication.mute();
        }

        // Opening a camera takes seconds, and on a first run it waits on the
        // macOS permission prompt. Doing that inline would park an executor
        // thread in the middle of connecting, so it goes to the blocking pool.
        let camera_source = match tokio::task::spawn_blocking(VideoSource::new).await {
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

        // Same `SelectAll` trick as the remote streams: an absent camera leaves
        // it empty, which reports as terminated instead of yielding forever.
        let mut camera_frames: SelectAll<BoxStream<'static, Arc<I420Buffer>>> = SelectAll::new();
        let mut camera: Option<NativeVideoSource> = None;

        if let Some(source) = &camera_source {
            camera = publish_camera(&room, source.width, source.height).await;
            camera_frames.push(camera_stream(source.frames()).boxed());
        }

        // Cancel-safe, and cheap: 10 ms is exactly one WebRTC frame.
        let mut ticker = tokio::time::interval(Duration::from_millis(10));

        // `Burst` is the default: once an iteration overruns 10 ms, the ticker
        // fires as fast as it can to catch up, and the branch wins the `select!`
        // over and over while it does. That spins a worker thread instead of
        // yielding to the camera, the remote streams, or iced's own redraws.
        // Draining the mic ring is idempotent, so a late tick can just be late.
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
                        RoomEvent::TrackSubscriptionFailed { error, track_sid, .. } => {
                            eprintln!("Track subscription failed for {track_sid}: {error}");
                        }
                        // Room events are infrequent, so logging the rest costs
                        // nothing and makes a track that never arrives visible
                        // instead of silently swallowed.
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

                    // The self-view is only ever displayed until a remote track
                    // arrives, so once one has, sending it up is pure waste: it
                    // burns a redraw that paints an unchanged remote frame and
                    // costs the remote stream a slot in a 4-deep channel.
                    if remote_frames == 0 {
                        // Sharing the remote counter keeps local and remote ids
                        // distinct, so the pipeline never mistakes one for a
                        // repeat of the other and skips the upload.
                        frame_id = frame_id.wrapping_add(1);

                        let _ = output.try_send(MeetingRoomMessage::LocalFrame(Frame::new(
                            buffer,
                            frame_id,
                            VideoRotation::VideoRotation0,
                        )));
                    }
                }
                // Cancel-safe, so losing the race to another branch just means
                // the change is picked up on the next pass.
                changed = controls.changed().fuse() => {
                    // Err means the page is gone; the stream is about to be
                    // dropped along with it.
                    if changed.is_err() { continue }

                    microphone_muted = controls.borrow_and_update().microphone_muted;

                    // Muting the publication rather than the track: both disable
                    // the RTC track, but this one also tells the server, so the
                    // others see a mute indicator instead of silence.
                    if let Some((_, _, publication)) = &mic {
                        if microphone_muted {
                            publication.mute();
                        } else {
                            publication.unmute();
                        }
                    }
                }
                _ = ticker.tick().fuse() => {
                    let (Some(source), Some((rtc, buffer, _))) = (&mut audio_source, &mut mic)
                        else { continue };

                    // Drain whatever the mic has queued; false = less than a full
                    // frame ready, so we wait for the next tick.
                    while source.pop_frame(buffer) {
                        // Drained even while muted — cpal keeps filling the ring
                        // regardless, so stopping here would back it up and make
                        // unmuting replay seconds of stale audio.
                        if microphone_muted { continue }

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
