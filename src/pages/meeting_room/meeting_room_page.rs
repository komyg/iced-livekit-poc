use std::collections::HashMap;
use std::sync::Arc;

use iced::futures::channel::mpsc::Sender;
use iced::futures::stream::{AbortHandle, BoxStream};
use iced::futures::{
    FutureExt, SinkExt, Stream, StreamExt, select, stream, stream::FuturesUnordered,
    stream::SelectAll,
};
use iced::widget::{container, row, shader, stack, text};
use iced::{Element, Length, Subscription, Task, padding};
use livekit::id::{ParticipantIdentity, TrackSid};
use livekit::options::TrackPublishOptions;
use livekit::participant::Participant;
use livekit::publication::LocalTrackPublication;
use livekit::track::RemoteTrack;
use livekit::track::TrackSource;
use livekit::track::{LocalAudioTrack, LocalTrack, LocalVideoTrack, TrackKind};
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::AudioSourceOptions;
use livekit::webrtc::audio_source::RtcAudioSource;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use livekit::webrtc::video_frame::{
    BoxVideoFrame, I420Buffer, VideoBuffer, VideoFrame, VideoRotation,
};
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_source::{RtcVideoSource, VideoResolution};
use livekit::webrtc::video_stream::native::NativeVideoStream;
use livekit::{DataPacket, Room, RoomEvent, RoomOptions};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::MissedTickBehavior;

use super::chat_notification::{ChatNotification, ChatNotificationAction, ChatNotificationMessage};
use super::data::{EVERYONE_ID, Recipient, Roster};
use super::featured_view;
use super::meeting_chat::{
    ChatEntry, ChatRequest, MeetingChat, MeetingChatAction, MeetingChatMessage,
};
use super::meeting_controls::{MeetingControls, MeetingControlsMessage};
use super::mosaic_view;
use crate::audio::audio_sink::AudioSink;
use crate::audio::audio_source::AudioSource;
use crate::video::video_sink::{Frame, to_i420};
use crate::video::video_source::VideoSource;

/// The chat protocol `OpenVidu` Meet speaks: a reliable data packet on this
/// topic with a `{"message": "..."}` JSON payload. Note this is neither of
/// `LiveKit`'s own chat mechanisms (`lk.chat` text streams or the legacy
/// `ChatMessage` packet) — clients talking those will not interop.
const OPENVIDU_CHAT_TOPIC: &str = "chat";

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
    Frame {
        identity: String,
        frame: Frame,
    },
    VideoEnded(String),
    Participants {
        local: (String, String),
        remote: Roster,
    },
    MeetingControls(MeetingControlsMessage),
    MeetingChat(MeetingChatMessage),
    ChatNotification(ChatNotificationMessage),
    MicrophonePublished(LocalTrackPublication),
    CameraControl(watch::Sender<bool>),
    ChatControl(mpsc::UnboundedSender<ChatRequest>),
    ChatReceived(ChatEntry),
}

pub struct MeetingRoomPage {
    token: String,
    url: String,
    status: Status,
    /// Our own (identity, label); `None` until the room loop reports in.
    local: Option<(String, String)>,
    roster: Roster,
    /// Latest frame per participant identity, ours included.
    frames: HashMap<String, Frame>,
    meeting_controls: MeetingControls,
    meeting_chat: MeetingChat,
    chat_notification: ChatNotification,
    microphone: Option<LocalTrackPublication>,
    camera_control: Option<watch::Sender<bool>>,
    chat_control: Option<mpsc::UnboundedSender<ChatRequest>>,
}

impl MeetingRoomPage {
    pub fn new(token: String, url: String) -> Self {
        Self {
            token,
            url,
            status: Status::Connecting,
            local: None,
            roster: Roster::new(),
            frames: HashMap::new(),
            meeting_controls: MeetingControls::new(),
            meeting_chat: MeetingChat::new(),
            chat_notification: ChatNotification::new(),
            microphone: None,
            camera_control: None,
            chat_control: None,
        }
    }

    pub fn view(&self) -> Element<'_, MeetingRoomMessage> {
        let tiles = mosaic_view::ordered_tiles(self.local.as_ref(), &self.roster, &self.frames);

        let content: Element<'_, MeetingRoomMessage> = if self.status.is_error() {
            text(self.status.label()).style(text::danger).into()
        } else if self.meeting_controls.mosaic_on && !tiles.is_empty() {
            mosaic_view::view(tiles)
        } else if let Some(single) = featured_view::view(&tiles) {
            single
        } else {
            text(self.status.label()).into()
        };

        let stage = stack![
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
        .extend(self.chat_notification.view().map(|toast| {
            container(toast.map(MeetingRoomMessage::ChatNotification))
                .center_x(Length::Fill)
                .align_bottom(Length::Fill)
                .padding(padding::bottom(110))
                .into()
        }));

        row![stage]
            .extend((!self.meeting_controls.chat_hidden).then(|| {
                self.meeting_chat
                    .view()
                    .map(MeetingRoomMessage::MeetingChat)
            }))
            .into()
    }

    pub fn update(&mut self, message: MeetingRoomMessage) -> Task<MeetingRoomMessage> {
        match message {
            MeetingRoomMessage::Status(status) => self.status = status,
            MeetingRoomMessage::Frame { identity, frame } => {
                // A frame already in flight when the camera was switched off
                // must not resurrect our tile.
                let ours = self.is_local(&identity);
                if !(ours && self.meeting_controls.camera_off) {
                    self.frames.insert(identity, frame);
                }
            }
            MeetingRoomMessage::VideoEnded(identity) => {
                self.frames.remove(&identity);
            }
            MeetingRoomMessage::Participants { local, remote } => {
                self.frames
                    .retain(|identity, _| *identity == local.0 || remote.contains_key(identity));
                self.local = Some(local);
                self.meeting_chat
                    .update(MeetingChatMessage::ParticipantsChanged(remote.clone()));
                self.roster = remote;
            }
            MeetingRoomMessage::MeetingControls(message) => {
                self.meeting_controls.update(message);
                self.apply_controls();

                if self.meeting_controls.camera_off
                    && let Some((identity, _)) = &self.local
                {
                    self.frames.remove(identity);
                }

                if !self.meeting_controls.chat_hidden {
                    self.chat_notification.dismiss();
                }
            }
            MeetingRoomMessage::MeetingChat(message) => {
                if let MeetingChatAction::Send(request) = self.meeting_chat.update(message)
                    && let Some(chat) = &self.chat_control
                    && chat.send(request).is_err()
                {
                    eprintln!("Chat unavailable: room loop is gone");
                }
            }
            MeetingRoomMessage::ChatNotification(message) => {
                match self.chat_notification.update(message) {
                    ChatNotificationAction::OpenChat => {
                        self.meeting_controls
                            .update(MeetingControlsMessage::ToggleChat);
                        self.chat_notification.dismiss();
                    }
                    ChatNotificationAction::Run(task) => {
                        return task.map(MeetingRoomMessage::ChatNotification);
                    }
                    ChatNotificationAction::None => {}
                }
            }
            MeetingRoomMessage::ChatReceived(entry) => {
                self.meeting_chat.push(entry.clone());
                if self.meeting_controls.chat_hidden {
                    return self.update(MeetingRoomMessage::ChatNotification(
                        ChatNotificationMessage::Show(entry),
                    ));
                }
            }
            MeetingRoomMessage::ChatControl(sender) => self.chat_control = Some(sender),
            MeetingRoomMessage::MicrophonePublished(publication) => {
                self.microphone = Some(publication);
                self.apply_controls();
            }
            MeetingRoomMessage::CameraControl(sender) => {
                self.camera_control = Some(sender);
                self.apply_controls();
            }
        }

        Task::none()
    }

    fn is_local(&self, identity: &str) -> bool {
        self.local
            .as_ref()
            .is_some_and(|(local, _)| local == identity)
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
            // Only notifies the room loop on a real change, so re-sends from
            // unrelated control presses don't wake it.
            let wanted = !self.meeting_controls.camera_off;
            camera.send_if_modified(|on| std::mem::replace(on, wanted) != wanted);
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

fn participant_label(name: String, identity: String) -> String {
    if name.is_empty() { identity } else { name }
}

/// Tells the page who is here. Reads our own name from the room every time,
/// so a local rename is picked up without tracking it separately.
async fn send_participants(output: &mut Sender<MeetingRoomMessage>, room: &Room, roster: &Roster) {
    let local = room.local_participant();

    let _ = output
        .send(MeetingRoomMessage::Participants {
            local: (
                local.identity().0,
                participant_label(local.name(), local.identity().0),
            ),
            remote: roster.clone(),
        })
        .await;
}

/// Adapts `LiveKit`'s participant map into the shape the chat panel wants.
fn roster_of(room: &Room) -> Roster {
    room.remote_participants()
        .into_iter()
        .map(|(identity, participant)| {
            (
                identity.0,
                participant_label(participant.name(), participant.identity().0),
            )
        })
        .collect()
}

#[expect(
    clippy::too_many_lines,
    reason = "one `select!` loop over the room's lifetime; splitting the arms \
              would scatter the shared connection state across helpers"
)]
fn connect(data: &(String, String)) -> impl Stream<Item = MeetingRoomMessage> + use<> {
    let (url, token) = data.clone();

    // A small buffer plus LiveKit's own keep-newest frame queue means a slow UI
    // drops stale frames at the source instead of accumulating latency. The
    // camera and every remote track share it, and `try_send` drops a frame
    // when it is full — that is the intended backpressure, not a bug. Every
    // frame that does get through triggers a redraw, so the redraw rate grows
    // with the head count; fine for a proof of concept.
    iced::stream::channel(8, async move |mut output| {
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

        let local_identity = room.local_participant().identity().0;

        // Anyone already here when we arrived fires no `ParticipantConnected`,
        // so the roster has to start from a snapshot rather than from events.
        let mut roster = roster_of(&room);
        send_participants(&mut output, &room, &roster).await;

        // Each remote stream is tagged with its participant so the page can
        // route the frame to the right tile.
        let mut videos: SelectAll<BoxStream<'static, (String, BoxVideoFrame)>> = SelectAll::new();
        let mut audio: SelectAll<NativeAudioStream> = SelectAll::new();
        let mut frame_id: u64 = 0;

        // One video stream per remote participant. `SelectAll` cannot remove
        // a stream by key, so each carries an abort handle instead.
        let mut remote_videos: HashMap<String, (TrackSid, AbortHandle)> = HashMap::new();

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
            .map(|source| (source.sample_rate, source.channels, source.frame_len));

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

        let (chat_tx, chat_rx) = mpsc::unbounded_channel::<ChatRequest>();
        let mut chat_requests = stream::unfold(chat_rx, async |mut receiver| {
            let request = receiver.recv().await?;
            Some((request, receiver))
        })
        .boxed()
        .fuse();

        let _ = output.send(MeetingRoomMessage::ChatControl(chat_tx)).await;

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

                    eprintln!("Room event: {event:#?}");
                    match event {
                        RoomEvent::TrackSubscribed {
                            track: RemoteTrack::Video(track),
                            publication,
                            participant,
                        } if publication.source() != TrackSource::Screenshare => {
                            let identity = participant.identity().0;

                            let (frames, abort) =
                                stream::abortable(NativeVideoStream::new(track.rtc_track()));
                            let tag = identity.clone();
                            videos.push(frames.map(move |frame| (tag.clone(), frame)).boxed());

                            // Latest wins: a reconnect can re-subscribe
                            // without an unsubscribe for the old track.
                            if let Some((_, previous)) =
                                remote_videos.insert(identity, (track.sid(), abort))
                            {
                                previous.abort();
                            }
                        }
                        RoomEvent::TrackUnsubscribed {
                            track: RemoteTrack::Video(track),
                            participant,
                            ..
                        } => {
                            let identity = participant.identity().0;

                            // A late unsubscribe for a track we already
                            // replaced must not kill its replacement.
                            let current = remote_videos
                                .get(&identity)
                                .is_some_and(|(sid, _)| *sid == track.sid());

                            if current {
                                if let Some((_, abort)) = remote_videos.remove(&identity) {
                                    abort.abort();
                                }

                                let _ = output
                                    .send(MeetingRoomMessage::VideoEnded(identity))
                                    .await;
                            }
                        }
                        RoomEvent::TrackMuted {
                            participant: Participant::Remote(participant),
                            publication,
                        } if publication.kind() == TrackKind::Video => {
                            // "Camera off" in the browser is a mute, not an
                            // unpublish: the track stays subscribed and just
                            // goes quiet, which would freeze the tile on its
                            // last frame. Frames resume by themselves on
                            // unmute.
                            let _ = output
                                .send(MeetingRoomMessage::VideoEnded(participant.identity().0))
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

                            // Joins and leaves that happened while we were down
                            // produced no events we could see, so trust the
                            // room over our accumulated roster.
                            roster = roster_of(&room);
                            send_participants(&mut output, &room, &roster).await;
                        }
                        RoomEvent::ParticipantConnected(participant) => {
                            roster.insert(
                                participant.identity().0,
                                participant_label(
                                    participant.name(),
                                    participant.identity().0,
                                ),
                            );

                            send_participants(&mut output, &room, &roster).await;
                        }
                        RoomEvent::ParticipantDisconnected(participant) => {
                            let identity = participant.identity().0;
                            roster.remove(&identity);

                            if let Some((_, abort)) = remote_videos.remove(&identity) {
                                abort.abort();
                            }

                            send_participants(&mut output, &room, &roster).await;
                        }
                        RoomEvent::ParticipantNameChanged { participant, name, .. } => {
                            let identity = participant.identity().0;

                            // The roster only ever holds remotes — adding
                            // ourselves would put us in our own recipient
                            // list. Our own rename still reaches the page,
                            // since `send_participants` reads it off the room.
                            if roster.contains_key(&identity) {
                                roster.insert(
                                    identity.clone(),
                                    participant_label(name, identity),
                                );
                            }

                            send_participants(&mut output, &room, &roster).await;
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
                        RoomEvent::DataReceived { payload, topic: Some(topic), participant, .. }
                            if topic == OPENVIDU_CHAT_TOPIC =>
                        {
                            // OpenVidu Meet chat, plus our own `to` extension:
                            // JSON `{"message": "...", "to": "<identity>"}`.
                            let payload =
                                serde_json::from_slice::<serde_json::Value>(&payload).ok();

                            let body = payload.as_ref().and_then(|value| {
                                value.get("message")?.as_str().map(ToOwned::to_owned)
                            });

                            let Some(body) = body else {
                                eprintln!("Unrecognized chat payload on topic {topic:?}");
                                continue;
                            };

                            let to = payload
                                .as_ref()
                                .and_then(|value| value.get("to")?.as_str())
                                .filter(|to| !to.is_empty())
                                .unwrap_or(EVERYONE_ID);

                            let local = room.local_participant();
                            let addressed_to_us = to == local.identity().0;

                            // Targeted packets already reach only their
                            // recipient, so this is a backstop against a client
                            // that sets `to` but still broadcasts.
                            if to != EVERYONE_ID && !addressed_to_us {
                                continue;
                            }

                            let recipient = if addressed_to_us {
                                Recipient {
                                    id: local.identity().0,
                                    label: participant_label(
                                        local.name(),
                                        local.identity().0,
                                    ),
                                }
                            } else {
                                Recipient::everyone()
                            };

                            let sender = participant.map_or_else(
                                || "Unknown".to_owned(),
                                |p| participant_label(p.name(), p.identity().0),
                            );

                            let _ = output
                                .send(MeetingRoomMessage::ChatReceived(ChatEntry {
                                    sender,
                                    recipient,
                                    body,
                                }))
                                .await;
                        }
                        _ => {}
                    }
                }
                frame = videos.next() => {
                    let Some((identity, frame)) = frame else { continue };

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

                    let _ = output.try_send(MeetingRoomMessage::Frame {
                        identity,
                        frame: Frame::new(Arc::new(buffer), frame_id, frame.rotation),
                    });
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

                    frame_id = frame_id.wrapping_add(1);

                    let _ = output.try_send(MeetingRoomMessage::Frame {
                        identity: local_identity.clone(),
                        frame: Frame::new(buffer, frame_id, VideoRotation::VideoRotation0),
                    });
                }
                camera_wanted = camera_toggles.next() => {
                    let Some(camera_wanted) = camera_wanted else { continue };

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
                request = chat_requests.next() => {
                    let Some(request) = request else { continue };

                    let packet = DataPacket {
                        // `to` is carried in the payload because the receiver
                        // never sees `destination_identities` — that is routing
                        // the server consumes — and it has to know whether the
                        // message was private in order to label it.
                        payload: serde_json::json!({
                            "message": request.body,
                            "to": request.recipient.id,
                        })
                            .to_string()
                            .into_bytes(),
                        topic: Some(OPENVIDU_CHAT_TOPIC.to_owned()),
                        reliable: true,
                        destination_identities: if request.recipient.is_everyone() {
                            Vec::new()
                        } else {
                            vec![ParticipantIdentity(request.recipient.id.clone())]
                        },
                    };

                    // The message is only shown once the server accepted it,
                    // so the log never contains anything that wasn't
                    // delivered. LiveKit does not echo our own packet back.
                    match room.local_participant().publish_data(packet).await {
                        Ok(()) => {
                            let local = room.local_participant();

                            let _ = output
                                .send(MeetingRoomMessage::ChatReceived(ChatEntry {
                                    sender: participant_label(local.name(), local.identity().0),
                                    recipient: request.recipient,
                                    body: request.body,
                                }))
                                .await;
                        }
                        Err(error) => eprintln!("Chat send failed: {error}"),
                    }
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
