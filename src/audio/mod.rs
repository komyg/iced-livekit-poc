pub mod audio_sink;
pub mod audio_source;

/// Divisor that maps an `i16` sample (−32768..=32767) onto cpal's f32 range
/// (−1.0..=1.0). It's 2^15 — the magnitude of `i16::MIN`.
const I16_TO_F32_SCALE: f32 = 32768.0;

/// Converts a normalized sample back to PCM16.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a float-to-int `as` saturates at i16's bounds, which is the wanted clipping"
)]
pub fn f32_to_i16(sample: f32) -> i16 {
    (sample * I16_TO_F32_SCALE) as i16
}
