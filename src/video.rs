//! Rendering remote video frames through iced's custom-shader widget.
//!
//! Frames stay in planar YUV all the way to the GPU: the three I420 planes are
//! uploaded as separate `R8Unorm` textures and converted to RGB in the fragment
//! shader. That keeps the CPU out of the colour conversion and moves 1.5 bytes
//! per pixel across the bus instead of RGBA's 4.

mod pipeline;

use std::sync::Arc;

use iced::wgpu;
use iced::widget::shader::{self, Primitive, Viewport};
use iced::{Rectangle, mouse};
use livekit::webrtc::video_frame::{
    I420Buffer, VideoBuffer, VideoBufferType, VideoRotation, native::VideoFrameBufferExt,
};

use pipeline::VideoPipeline;

/// One decoded frame, ready to hand to the GPU.
///
/// [`I420Buffer`] is already `Debug + Send + Sync`, which is exactly what a
/// shader primitive needs, so the planes travel by `Arc` and are never copied
/// on the way through.
#[derive(Clone, Debug)]
pub struct Frame {
    buffer: Arc<I420Buffer>,
    id: u64,
    rotation: VideoRotation,
}

impl Frame {
    pub const fn new(buffer: Arc<I420Buffer>, id: u64, rotation: VideoRotation) -> Self {
        Self {
            buffer,
            id,
            rotation,
        }
    }
}

/// Normalises any incoming frame buffer to I420.
///
/// `VideoFrameBufferExt` has a blanket impl over sized `VideoBuffer`s, so it
/// does not apply to `dyn VideoBuffer` — hence the trip through each concrete
/// accessor. Hardware decoders hand back `Native` (a `CVPixelBuffer` on macOS)
/// or `NV12`; software VP8/VP9 gives `I420` directly and the conversion is a
/// cheap refcount bump.
pub fn to_i420(buffer: &dyn VideoBuffer) -> Option<I420Buffer> {
    match buffer.buffer_type() {
        VideoBufferType::I420 => buffer.as_i420().map(VideoFrameBufferExt::to_i420),
        VideoBufferType::I420A => buffer.as_i420a().map(VideoFrameBufferExt::to_i420),
        VideoBufferType::I422 => buffer.as_i422().map(VideoFrameBufferExt::to_i420),
        VideoBufferType::I444 => buffer.as_i444().map(VideoFrameBufferExt::to_i420),
        VideoBufferType::I010 => buffer.as_i010().map(VideoFrameBufferExt::to_i420),
        VideoBufferType::NV12 => buffer.as_nv12().map(VideoFrameBufferExt::to_i420),
        VideoBufferType::Native => buffer.as_native().map(VideoFrameBufferExt::to_i420),
        // `VideoBufferType` is #[non_exhaustive].
        _ => None,
    }
}

/// Draws a [`Frame`]. Emits no messages, so it is generic over `Message`.
#[derive(Debug)]
pub struct VideoSink {
    frame: Frame,
}

impl VideoSink {
    pub const fn new(frame: Frame) -> Self {
        Self { frame }
    }
}

impl<Message> shader::Program<Message> for VideoSink {
    type State = ();
    type Primitive = VideoPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        VideoPrimitive {
            buffer: Arc::clone(&self.frame.buffer),
            id: self.frame.id,
            rotation: quarter_turns(self.frame.rotation),
        }
    }
}

#[derive(Debug)]
pub struct VideoPrimitive {
    buffer: Arc<I420Buffer>,
    id: u64,
    rotation: u32,
}

impl Primitive for VideoPrimitive {
    type Pipeline = VideoPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        // Unlike `draw`, `prepare` is called for every primitive regardless of
        // whether it intersects the visible area, so degenerate bounds reach us
        // when the window is minimised or the widget is scrolled off-screen.
        // Letting those through would put NaN in the letterbox uniform.
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return;
        }

        // A zero extent would panic inside `create_texture`.
        if self.buffer.width() == 0 || self.buffer.height() == 0 {
            return;
        }

        pipeline.upload(device, queue, &self.buffer, self.id, self.rotation, *bounds);
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        // Returning true reuses iced's render pass, whose viewport and scissor
        // are already set to our bounds — so `render` is never called.
        pipeline.draw(render_pass)
    }
}

const fn quarter_turns(rotation: VideoRotation) -> u32 {
    match rotation {
        VideoRotation::VideoRotation0 => 0,
        VideoRotation::VideoRotation90 => 1,
        VideoRotation::VideoRotation180 => 2,
        VideoRotation::VideoRotation270 => 3,
    }
}
