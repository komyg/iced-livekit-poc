use std::thread::JoinHandle;
use std::{sync::mpsc, thread};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, RingBuffer};

pub struct AudioSource {
    consumer: Consumer<i16>,
    pub sample_rate: u32,
    pub channels: u16,
    pub frame_len: usize,
    shutdown: Option<mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

struct AudioInput {
    consumer: Consumer<i16>,
    sample_rate: u32,
    channels: u16,
    frame_len: usize,
}

impl AudioSource {
    pub fn new() -> Result<Self, String> {
        let (init_tx, init_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

        let handle = thread::Builder::new()
            .name("audio-input".to_string())
            .spawn(move || audio_input_thread(&init_tx, &shutdown_rx))
            .map_err(|e| e.to_string())?;

        init_rx
            .recv()
            .map_err(|_| "Audio thread exited before init".to_string())?
            .map(|audio_input| Self {
                consumer: audio_input.consumer,
                sample_rate: audio_input.sample_rate,
                channels: audio_input.channels,
                frame_len: audio_input.frame_len,
                shutdown: Some(shutdown_tx),
                handle: Some(handle),
            })
    }

    /// Fills `out` with one 10 ms frame. Returns `false` if the mic hasn't
    /// buffered a whole frame yet, leaving `out` untouched.
    pub fn pop_frame(&mut self, out: &mut Vec<i16>) -> bool {
        // `read_chunk` fails unless all `frame_len` samples are readable, so a
        // partial frame can never reach the caller.
        let Ok(chunk) = self.consumer.read_chunk(self.frame_len) else {
            return false;
        };

        // The ring may wrap mid-frame; `second` is empty when it doesn't.
        let (first, second) = chunk.as_slices();
        out.clear();
        out.extend_from_slice(first);
        out.extend_from_slice(second);

        chunk.commit_all();
        true
    }
}

impl Drop for AudioSource {
    fn drop(&mut self) {
        drop(self.shutdown.take());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn audio_input_thread(
    init_tx: &mpsc::Sender<Result<AudioInput, String>>,
    shutdown: &mpsc::Receiver<()>,
) {
    match build_input_stream() {
        Ok((stream, audio_input)) => {
            if init_tx.send(Ok(audio_input)).is_err() {
                return;
            }
            let _ = shutdown.recv();
            drop(stream);
        }
        Err(error) => {
            let _ = init_tx.send(Err(error));
        }
    }
}

fn build_input_stream() -> Result<(cpal::Stream, AudioInput), String> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("No default input device".to_string())?;
    let config = device.default_input_config().map_err(|e| e.to_string())?;

    let sample_rate = config.sample_rate();
    let channels = config.channels();

    let slots = sample_rate
        .saturating_mul(u32::from(channels))
        .saturating_mul(200)
        .checked_div(1000)
        .ok_or("Overflow in audio input buffer capacity calculation".to_string())?;
    let capacity = usize::try_from(slots).map_err(|e| e.to_string())?;
    let (mut producer, consumer) = RingBuffer::<i16>::new(capacity);

    // Samples per 10 ms frame, interleaved. Fixed for the life of the stream,
    // so the fallible arithmetic happens once, here.
    let samples = sample_rate
        .checked_div(100)
        .and_then(|per_channel| per_channel.checked_mul(u32::from(channels)))
        .ok_or("Overflow in audio frame length calculation".to_string())?;
    let frame_len = usize::try_from(samples).map_err(|e| e.to_string())?;

    let stream = device
        .build_input_stream(
            config.config(),
            move |input: &[f32], _: &cpal::InputCallbackInfo| {
                for &sample in input {
                    if producer.push(super::f32_to_i16(sample)).is_err() {
                        break;
                    }
                }
            },
            |error| eprintln!("Audio input error: {error}"),
            None,
        )
        .map_err(|e| e.to_string())?;

    stream.play().map_err(|e| e.to_string())?;
    Ok((
        stream,
        AudioInput {
            consumer,
            sample_rate,
            channels,
            frame_len,
        },
    ))
}
