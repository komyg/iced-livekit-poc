# iced-pv-meet-poc

A proof of concept video-call client built with [iced](https://iced.rs) and
[LiveKit](https://livekit.io). It captures the local camera and microphone,
publishes them to a LiveKit room, and renders the remote participant's video
through a custom `wgpu` shader.

## What it does

- **Login screen** — takes LiveKit API key, secret, URL, identity and room, then
  mints a join token locally with `livekit-api`. Fields are pre-filled from the
  environment.
- **Meeting room** — connects to the room, publishes camera and mic tracks, and
  subscribes to remote tracks. Shows connection status and a self-view.
- **Video** — camera capture via `nokhwa` (AVFoundation), converted to I420 for
  WebRTC. Incoming frames are uploaded as YUV planes and converted to RGB on the
  GPU by `src/video/yuv.wgsl`, so no per-pixel work happens on the CPU.
- **Audio** — capture and playback via `cpal`, bridged to WebRTC's fixed 10 ms
  frames through an `rtrb` lock-free ring buffer.

## Running

```sh
cp .env.example .env   # then edit if you are not using the dev server
cargo run
```

`.env` is read at startup to pre-fill the login form; every field can still be
edited in the UI. It is gitignored — keep real credentials out of the repo.

| Variable | Meaning |
| --- | --- |
| `LIVEKIT_URL` | WebSocket URL of the LiveKit server |
| `LIVEKIT_API_KEY` | API key |
| `LIVEKIT_API_SECRET` | API secret |
| `LIVEKIT_ROOM` | Room to join |
| `LIVEKIT_IDENTITY` | Participant identity and display name |

To see what the render and transport layers are doing:

```sh
RUST_LOG=info cargo run
```
