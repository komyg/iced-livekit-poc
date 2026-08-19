pub mod audio_sink;
pub mod audio_source;

/// Divisor that maps an `i16` sample (−32768..=32767) onto cpal's f32 range
/// (−1.0..=1.0). It's 2^15 — the magnitude of `i16::MIN`.
const I16_TO_F32_SCALE: f32 = 32768.0;
