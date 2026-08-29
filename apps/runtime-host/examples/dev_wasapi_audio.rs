#[path = "../src/audio_output/mod.rs"]
mod audio_output;

#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use futures_util::stream;
#[cfg(windows)]
use npc_runtime_core::{AudioChunk, AudioSink, SpeechStreamItem, TurnIdentity};
#[cfg(windows)]
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_path = std::env::args_os()
        .nth(1)
        .ok_or("usage: dev_wasapi_audio <24-khz-mono-s16le.raw>")?;
    let pcm_s16le = tokio::fs::read(raw_path).await?;
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
    let sink = audio_output::DevWasapiAudioSink::new(audio_output::DevWasapiConfig::default())?;
    let receipt = sink
        .play(&identity, 1, Box::pin(stream), CancellationToken::new())
        .await?;
    if !receipt.completed {
        return Err("playback did not drain to completion".into());
    }
    if receipt.duration == Duration::ZERO {
        return Err("input contained no PCM frames".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
