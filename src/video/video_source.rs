use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};

use livekit::webrtc::native::yuv_helper;
use livekit::webrtc::video_frame::I420Buffer;
use nokhwa::Camera;
use nokhwa::pixel_format::RgbAFormat;
use nokhwa::utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType};
use tokio::sync::watch;

const PREFERRED_WIDTH: u32 = 1280;
const PREFERRED_HEIGHT: u32 = 720;
const PREFERRED_FPS: u32 = 30;

const RGBA_BYTES_PER_PIXEL: u32 = 4;

pub struct VideoSource {
    frames: watch::Receiver<Option<Arc<I420Buffer>>>,
    pub width: u32,
    pub height: u32,
    shutdown: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

struct VideoInput {
    width: u32,
    height: u32,
}

impl VideoSource {
    pub fn new() -> Result<Self, String> {
        let (init_tx, init_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let (frames_tx, frames_rx) = watch::channel(None);

        let handle = thread::Builder::new()
            .name("camera-input".to_string())
            .spawn(move || video_input_thread(&init_tx, &shutdown_rx, &frames_tx))
            .map_err(|e| e.to_string())?;

        init_rx
            .recv()
            .map_err(|_| "Camera thread exited before init".to_string())?
            .map(|input| Self {
                frames: frames_rx,
                width: input.width,
                height: input.height,
                shutdown: Some(shutdown_tx),
                handle: Some(handle),
            })
    }

    pub fn frames(&self) -> watch::Receiver<Option<Arc<I420Buffer>>> {
        self.frames.clone()
    }
}

impl Drop for VideoSource {
    fn drop(&mut self) {
        drop(self.shutdown.take());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn video_input_thread(
    init_tx: &Sender<Result<VideoInput, String>>,
    shutdown: &Receiver<()>,
    frames: &watch::Sender<Option<Arc<I420Buffer>>>,
) {
    let mut camera = match build_video_input() {
        Ok(camera) => camera,
        Err(error) => {
            let _ = init_tx.send(Err(error));
            return;
        }
    };

    let resolution = camera.resolution();

    // I420 subsamples chroma 2x2, so odd dimensions have no valid encoding.
    // Cameras hand back even sizes in practice; round down rather than fail.
    let width = resolution.width_x & !1;
    let height = resolution.height_y & !1;

    let Some(rgba_len) = rgba_buffer_len(width, height) else {
        let _ = init_tx.send(Err(format!(
            "Camera reported an unusable resolution: {}x{}",
            resolution.width_x, resolution.height_y
        )));
        return;
    };

    if init_tx.send(Ok(VideoInput { width, height })).is_err() {
        return;
    }

    // Reused across frames; the I420 buffer cannot be, since WebRTC keeps a
    // reference to every frame we capture.
    let mut rgba = vec![0u8; rgba_len];

    loop {
        // The capture below blocks, so shutdown is polled rather than awaited.
        if matches!(shutdown.try_recv(), Err(TryRecvError::Disconnected)) {
            break;
        }

        let Ok(buffer) = camera
            .frame()
            .inspect_err(|error| eprintln!("Camera capture failed: {error}"))
        else {
            break;
        };

        // The camera can renegotiate its format mid-stream; a frame that no
        // longer matches our scratch buffer is dropped rather than mis-decoded.
        if buffer.resolution().width_x != width || buffer.resolution().height_y != height {
            continue;
        }

        if let Err(error) = buffer.decode_image_to_buffer::<RgbAFormat>(&mut rgba) {
            eprintln!("Camera frame decode failed: {error}");
            continue;
        }

        let Some(i420) = rgba_to_i420(&rgba, width, height) else {
            continue;
        };

        // Fails only once every receiver is gone, which means nobody is left to
        // send to.
        if frames.send(Some(Arc::new(i420))).is_err() {
            break;
        }
    }

    let _ = camera.stop_stream();
}

fn build_video_input() -> Result<Camera, String> {
    request_permission()?;

    let preferred =
        RequestedFormat::new::<RgbAFormat>(RequestedFormatType::Closest(CameraFormat::new_from(
            PREFERRED_WIDTH,
            PREFERRED_HEIGHT,
            FrameFormat::YUYV,
            PREFERRED_FPS,
        )));

    // `Closest` filters on an exact frame-format match, so it yields nothing on
    // a device that reports no YUYV modes at all. Take whatever it offers then.
    let mut camera = Camera::new(CameraIndex::Index(0), preferred).or_else(|error| {
        eprintln!("Preferred camera format unavailable ({error}); falling back");
        Camera::new(
            CameraIndex::Index(0),
            RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestFrameRate),
        )
        .map_err(|e| e.to_string())
    })?;

    camera.open_stream().map_err(|e| e.to_string())?;
    Ok(camera)
}

fn request_permission() -> Result<(), String> {
    if nokhwa::nokhwa_check() {
        return Ok(());
    }

    let (tx, rx) = mpsc::channel();
    nokhwa::nokhwa_initialize(move |granted| {
        let _ = tx.send(granted);
    });

    match rx.recv() {
        Ok(true) => Ok(()),
        Ok(false) => Err("Camera permission denied".to_string()),
        Err(_) => Err("Camera permission request never completed".to_string()),
    }
}

/// `None` for a resolution that is degenerate or too large to address.
fn rgba_buffer_len(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }

    usize::try_from(width)
        .ok()
        .zip(usize::try_from(height).ok())
        .and_then(|(w, h)| w.checked_mul(h))
        .zip(usize::try_from(RGBA_BYTES_PER_PIXEL).ok())
        .and_then(|(pixels, bytes)| pixels.checked_mul(bytes))
}

/// Packs RGBA into a fresh I420 buffer.
///
/// libyuv names formats by little-endian word order, so its `ABGR` is the byte
/// order R, G, B, A — exactly what [`RgbAFormat`] produces. If red and blue come
/// out swapped, `argb_to_i420` is the other half of that naming trap.
fn rgba_to_i420(rgba: &[u8], width: u32, height: u32) -> Option<I420Buffer> {
    let (Ok(signed_width), Ok(signed_height)) = (i32::try_from(width), i32::try_from(height))
    else {
        return None;
    };

    let rgba_stride = width.checked_mul(RGBA_BYTES_PER_PIXEL)?;

    // The wrapper picks the tightest strides for us, so read them back rather
    // than assuming they equal the width.
    let mut buffer = I420Buffer::new(width, height);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (data_y, data_u, data_v) = buffer.data_mut();

    yuv_helper::abgr_to_i420(
        rgba,
        rgba_stride,
        data_y,
        stride_y,
        data_u,
        stride_u,
        data_v,
        stride_v,
        signed_width,
        signed_height,
    );

    Some(buffer)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions"
)]
mod tests {
    use super::{RGBA_BYTES_PER_PIXEL, rgba_buffer_len, rgba_to_i420};

    const WIDTH: u32 = 8;
    const HEIGHT: u32 = 8;

    /// Converts a solid-colour image and returns its top-left Y, U and V.
    fn convert(pixel: [u8; 4]) -> (u8, u8, u8) {
        let len = rgba_buffer_len(WIDTH, HEIGHT).unwrap();
        let rgba: Vec<u8> = pixel.iter().copied().cycle().take(len).collect();

        let buffer = rgba_to_i420(&rgba, WIDTH, HEIGHT).unwrap();
        let (y, u, v) = buffer.data();

        (y[0], u[0], v[0])
    }

    /// BT.601 puts pure red at V≈240 with U low, and pure blue at U≈240 with V
    /// low. Reading those back the right way round is what proves we picked the
    /// correct half of libyuv's `ABGR`/`ARGB` naming.
    #[test]
    fn red_rgba_lands_on_high_v() {
        let (y, u, v) = convert([255, 0, 0, 255]);

        assert!((75..=90).contains(&y), "luma for red was {y}");
        assert!(u < 100, "red should not raise U, got {u}");
        assert!(v > 200, "red should raise V, got {v}");
    }

    #[test]
    fn blue_rgba_lands_on_high_u() {
        let (y, u, v) = convert([0, 0, 255, 255]);

        assert!((25..=45).contains(&y), "luma for blue was {y}");
        assert!(u > 200, "blue should raise U, got {u}");
        assert!(v < 130, "blue should not raise V, got {v}");
    }

    #[test]
    fn rejects_degenerate_resolutions() {
        assert!(rgba_buffer_len(0, HEIGHT).is_none());
        assert!(rgba_buffer_len(WIDTH, 0).is_none());
        assert_eq!(
            rgba_buffer_len(2, 2),
            Some(4 * usize::try_from(RGBA_BYTES_PER_PIXEL).unwrap())
        );
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "temporary bench"
)]
mod bench {
    use super::{build_video_input, rgba_buffer_len, rgba_to_i420};
    use nokhwa::pixel_format::RgbAFormat;
    use std::time::{Duration, Instant};

    const FRAMES: u32 = 60;

    #[test]
    #[ignore = "needs a camera; run explicitly"]
    fn capture_pipeline_timing() {
        let mut camera = build_video_input().unwrap();
        println!("negotiated format: {:?}", camera.camera_format());

        let resolution = camera.resolution();
        let width = resolution.width_x & !1;
        let height = resolution.height_y & !1;
        let mut rgba = vec![0u8; rgba_buffer_len(width, height).unwrap()];

        for _ in 0..10 {
            let _ = camera.frame();
        }

        let (mut grab, mut decode, mut convert) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
        let start = Instant::now();

        for _ in 0..FRAMES {
            let mark = Instant::now();
            let buffer = camera.frame().unwrap();
            grab += mark.elapsed();

            let mark = Instant::now();
            buffer
                .decode_image_to_buffer::<RgbAFormat>(&mut rgba)
                .unwrap();
            decode += mark.elapsed();

            let mark = Instant::now();
            let _i420 = rgba_to_i420(&rgba, width, height).unwrap();
            convert += mark.elapsed();
        }

        let total = start.elapsed();
        println!(
            "{FRAMES} frames in {total:?} => {:.1} fps",
            f64::from(FRAMES) / total.as_secs_f64()
        );
        println!("  camera.frame():  {:?}", grab / FRAMES);
        println!("  YUYV -> RGBA:    {:?}", decode / FRAMES);
        println!("  RGBA -> I420:    {:?}", convert / FRAMES);
    }
}
