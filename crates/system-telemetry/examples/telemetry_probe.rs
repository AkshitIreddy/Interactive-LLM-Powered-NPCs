#![allow(clippy::print_stdout)]

use npc_system_telemetry::{collect, TelemetryRequest};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let selected_game_pid = std::env::args()
        .nth(1)
        .map(|value| value.parse())
        .transpose()?;
    let snapshot = collect(TelemetryRequest {
        selected_game_pid,
        ..TelemetryRequest::default()
    });
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}
