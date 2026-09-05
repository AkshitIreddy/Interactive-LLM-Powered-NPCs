use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

pub const MAX_CONTROL_BYTES: usize = 64 * 1024;
pub const MAX_PCM_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerCommand {
    Synthesize {
        request_id: String,
        generation: u64,
        text: String,
        voice_id: String,
    },
    Cancel {
        request_id: String,
        generation: u64,
    },
    Shutdown {
        request_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerEvent {
    pub event: String,
    pub request_id: String,
    pub sequence: u64,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub end_of_stream: bool,
    #[serde(default)]
    pub code: Option<String>,
}

pub fn write_blocking<T: Serialize>(
    writer: &mut impl Write,
    header: &T,
    pcm: &[u8],
) -> io::Result<()> {
    let encoded = serde_json::to_vec(header)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if encoded.len() > MAX_CONTROL_BYTES || pcm.len() > MAX_PCM_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    writer.write_all(&(encoded.len() as u32).to_be_bytes())?;
    writer.write_all(&(pcm.len() as u32).to_be_bytes())?;
    writer.write_all(&encoded)?;
    writer.write_all(pcm)?;
    writer.flush()
}

pub fn read_command_blocking(reader: &mut impl Read) -> io::Result<Option<WorkerCommand>> {
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_CONTROL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid control length",
        ));
    }
    let mut encoded = vec![0_u8; length];
    reader.read_exact(&mut encoded)?;
    serde_json::from_slice(&encoded)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_frame_is_bounded_and_round_trips() {
        let command = WorkerCommand::Cancel {
            request_id: "cancel-1".into(),
            generation: 8,
        };
        let encoded = serde_json::to_vec(&command).expect("encode");
        let mut frame = Vec::new();
        frame.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
        frame.extend_from_slice(&encoded);
        let parsed = read_command_blocking(&mut frame.as_slice())
            .expect("parse")
            .expect("command");
        assert!(matches!(
            parsed,
            WorkerCommand::Cancel {
                request_id,
                generation: 8
            } if request_id == "cancel-1"
        ));
    }
}
