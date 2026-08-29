#[path = "../src/audio_output/mod.rs"]
mod audio_output;

#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use futures_util::stream;
#[cfg(windows)]
use npc_runtime_core::{AudioChunk, SpeechStreamItem, TurnIdentity};
#[cfg(windows)]
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let usage = "usage: dev_wasapi_audio [--device <exact-name>] <24-khz-mono-s16le.raw>";
    let mut args = std::env::args_os().skip(1);
    let first = args.next().ok_or(usage)?;
    let (device_name, raw_path) = if first == "--device" {
        let name = args.next().ok_or(usage)?;
        let path = args.next().ok_or(usage)?;
        (Some(name.to_string_lossy().into_owned()), path)
    } else {
        (None, first)
    };
    if args.next().is_some() {
        return Err(usage.into());
    }
    let raw_path = std::path::PathBuf::from(raw_path);
    if !raw_path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("raw"))
    {
        return Err("input must be a headerless .raw S16LE file, not WAV/MP3".into());
    }
    let pcm_s16le = tokio::fs::read(&raw_path).await?;
    if pcm_s16le.starts_with(b"RIFF") || !pcm_s16le.len().is_multiple_of(2) {
        return Err("input is not headerless 24 kHz mono S16LE PCM".into());
    }
    let stream = stream::iter(vec![Ok(SpeechStreamItem::Audio(AudioChunk {
        sequence: 1,
        sample_rate_hz: audio_output::INPUT_SAMPLE_RATE_HZ,
        channels: audio_output::INPUT_CHANNELS,
        pcm_s16le,
        end_of_stream: true,
    }))]);
    let identity = TurnIdentity {
        session_id: "dev-wasapi".into(),
        turn_id: "manual-qualification".into(),
        cancellation_generation: 0,
    };
    let sink = audio_output::DevWasapiAudioSink::new(audio_output::DevWasapiConfig {
        device_name,
        ..audio_output::DevWasapiConfig::default()
    })?;
    let receipt = sink
        .play_submitted(&identity, 1, Box::pin(stream), CancellationToken::new())
        .await?;
    sink.stop(&identity).await?;
    if !receipt.completed {
        return Err("PCM was not fully submitted to WASAPI callbacks".into());
    }
    if receipt.source_duration == Duration::ZERO {
        return Err("input contained no PCM frames".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
