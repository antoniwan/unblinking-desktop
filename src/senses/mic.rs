//! Hearing: the default microphone, reduced to one loudness number.
//! Nothing is recorded; each buffer of samples is gone once it's measured.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// The open ears. Dropping them stops the stream and frees the mic.
pub struct Ears {
    // Kept only so it stays alive: dropping a cpal Stream stops it.
    _stream: cpal::Stream,
    // The audio thread writes the loudness, the main loop reads it.
    // There's no atomic f32, so the f32's bits are stored in an AtomicU32.
    level: Arc<AtomicU32>,
}

impl Ears {
    /// Opens the default input device. `Err` says why there is no hearing.
    pub fn open() -> Result<Ears, String> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("no microphone")?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let level = Arc::new(AtomicU32::new(0));

        // cpal calls our closure on its own real-time audio thread, many times
        // a second. It must be quick and must not wait on locks, so it only
        // does arithmetic and one atomic store.
        let sink = Arc::clone(&level);
        let on_error = |err| eprintln!("hearing error: {err}");
        let config = supported.config();
        // Mics deliver different sample types; generics let one function
        // (`loudness`) handle each, after converting to f32.
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], _: &_| store(&sink, loudness(data.iter().copied())),
                on_error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                config,
                move |data: &[i16], _: &_| {
                    store(
                        &sink,
                        loudness(data.iter().map(|&s| s as f32 / i16::MAX as f32)),
                    )
                },
                on_error,
                None,
            ),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        eprintln!(
            "hearing: {} Hz, {} channels",
            supported.sample_rate(),
            supported.channels()
        );
        Ok(Ears {
            _stream: stream,
            level,
        })
    }

    /// Loudness 0..1, as of the latest buffer.
    pub fn level(&self) -> f64 {
        f32::from_bits(self.level.load(Ordering::Relaxed)) as f64
    }
}

fn store(level: &AtomicU32, value: f32) {
    level.store(value.to_bits(), Ordering::Relaxed);
}

/// Loudness 0..1 on a decibel scale: about -55 dB is silence, -12 dB is a
/// shout. The same formula the Electron app used.
///
/// `impl Iterator<Item = f32>` accepts any sequence of f32s without
/// collecting them into a Vec first, so nothing is allocated here.
fn loudness(samples: impl Iterator<Item = f32>) -> f32 {
    let (mut sum, mut n) = (0.0f32, 0u32);
    for s in samples {
        sum += s * s;
        n += 1;
    }
    if n == 0 {
        return 0.0;
    }
    let rms = (sum / n as f32).sqrt();
    let db = 20.0 * (rms + 1e-9).log10();
    ((db + 55.0) / 43.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_zero_and_a_shout_is_one() {
        assert_eq!(loudness([0.0; 512].into_iter()), 0.0);
        assert_eq!(loudness([0.5, -0.5].into_iter().cycle().take(512)), 1.0);
    }

    #[test]
    fn talking_lands_in_between() {
        // An RMS of 0.01 is -40 dB: (−40 + 55) / 43 ≈ 0.35.
        let l = loudness([0.01, -0.01].into_iter().cycle().take(512));
        assert!((l - 15.0 / 43.0).abs() < 0.01, "{l}");
    }
}
