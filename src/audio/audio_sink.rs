use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use livekit::webrtc::audio_frame::AudioFrame;
use rtrb::{Producer, RingBuffer};

pub struct AudioSink {
    producer: Producer<i16>,
    pub sample_rate: u32,
    pub channels: u16,
    shutdown: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

struct AudioOutput {
    producer: Producer<i16>,
    sample_rate: u32,
    channels: u16,
}

impl AudioSink {
    /// Spawns the output thread, which opens the default device and reports its
    /// own rate/channels so the caller can ask WebRTC for that exact format and
    /// skip resampling.
    pub fn new() -> Result<Self, String> {
        let (init_tx, init_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

        // A dedicated OS thread owns the `!Send` cpal stream: it can't live in
        // the async task, and on Windows WASAPI needs stream build/use pinned to
        // one COM-initialised thread.
        let handle = thread::Builder::new()
            .name("audio-output".to_string())
            .spawn(move || audio_output_thread(&init_tx, &shutdown_rx))
            .map_err(|e| e.to_string())?;

        // Block until the thread reports its device format (or an error).
        init_rx
            .recv()
            .map_err(|_| "Audio thread exited before init".to_string())?
            .map(|audio_output| Self {
                producer: audio_output.producer,
                sample_rate: audio_output.sample_rate,
                channels: audio_output.channels,
                shutdown: Some(shutdown_tx),
                handle: Some(handle),
            })
    }

    pub fn push(&mut self, frame: &AudioFrame) {
        let free = self.producer.slots();
        let take = frame.data.len().min(free);
        let Ok(chunk) = self.producer.write_chunk_uninit(take) else {
            eprintln!("Audio buffer overflow");
            return;
        };

        chunk.fill_from_iter(frame.data.iter().copied());
    }
}

impl Drop for AudioSink {
    fn drop(&mut self) {
        // Drop the sender first so the thread's `recv` wakes and releases the
        // stream, *then* join — reversing this order deadlocks.
        drop(self.shutdown.take());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Owns the cpal stream for its whole lifetime: builds it, reports the format
/// back, then parks until the sink drops and the stream drops with it.
fn audio_output_thread(
    init_tx: &mpsc::Sender<Result<AudioOutput, String>>,
    shutdown: &mpsc::Receiver<()>,
) {
    match build_output_stream() {
        Ok((stream, audio_output)) => {
            // If the receiver is already gone, the sink was dropped; bail before
            // holding a stream nobody can feed.
            if init_tx.send(Ok(audio_output)).is_err() {
                return;
            }
            let _ = shutdown.recv(); // hold `stream` alive until the sink drops
            drop(stream);
        }
        Err(error) => {
            let _ = init_tx.send(Err(error));
        }
    }
}

fn build_output_stream() -> Result<(cpal::Stream, AudioOutput), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("No default output device".to_string())?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;

    let sample_rate = config.sample_rate();
    let channels = config.channels();

    // ~200 ms of interleaved samples.
    let slots = sample_rate
        .saturating_mul(u32::from(channels))
        .saturating_mul(200)
        .checked_div(1000)
        .ok_or("Overflow in audio output buffer capacity calculation".to_string())?;
    let capacity = usize::try_from(slots).map_err(|e| e.to_string())?;
    let (producer, mut consumer) = RingBuffer::<i16>::new(capacity);

    let stream = device
        .build_output_stream(
            config.config(),
            move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // "For every slot the sound card asked for, pull one sample off the ring and rescale it to f32; if the ring is dry, play silence."
                for sample in out.iter_mut() {
                    *sample = consumer
                        .pop()
                        .map_or(0.0, |value| f32::from(value) / super::I16_TO_F32_SCALE);
                }
            },
            |error| eprintln!("Audio output error: {error}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;

    Ok((
        stream,
        AudioOutput {
            producer,
            sample_rate,
            channels,
        },
    ))
}
