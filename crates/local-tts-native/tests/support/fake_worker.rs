use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use npc_local_tts_native::{
    wire::{read_command_blocking, write_blocking, WorkerCommand, WorkerEvent},
    OUTPUT_SAMPLE_RATE_HZ,
};

fn event(name: &str, request_id: &str, sequence: u64, end_of_stream: bool) -> WorkerEvent {
    WorkerEvent {
        event: name.into(),
        request_id: request_id.into(),
        sequence,
        sample_rate_hz: OUTPUT_SAMPLE_RATE_HZ,
        channels: 1,
        end_of_stream,
        code: None,
    }
}

fn emit(
    output: &mut BufWriter<io::Stdout>,
    name: &str,
    request_id: &str,
    sequence: u64,
    end_of_stream: bool,
    pcm: &[u8],
) -> io::Result<()> {
    write_blocking(
        output,
        &event(name, request_id, sequence, end_of_stream),
        pcm,
    )
}

fn marker(root: &Path, name: &str) -> io::Result<bool> {
    let marker = root.join(name);
    match OpenOptions::new().write(true).create_new(true).open(marker) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

fn pack_root() -> io::Result<PathBuf> {
    let mut args = env::args();
    while let Some(arg) = args.next() {
        if arg == "--pack-root" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| io::Error::other("missing pack root"));
        }
    }
    Err(io::Error::other("missing pack root"))
}

fn emit_healthy(output: &mut BufWriter<io::Stdout>, request_id: &str) -> io::Result<()> {
    emit(output, "audio", request_id, 0, false, &[1, 0, 2, 0])?;
    emit(output, "audio", request_id, 1, false, &[3, 0, 4, 0])?;
    emit(output, "completed", request_id, 2, true, &[])
}

fn run() -> io::Result<()> {
    let root = pack_root()?;
    fs::create_dir_all(&root)?;
    let mut output = BufWriter::new(io::stdout());
    emit(&mut output, "ready", "startup", 0, false, &[])?;
    let mut input = BufReader::new(io::stdin());
    let mut active: Option<(String, String)> = None;
    while let Some(command) = read_command_blocking(&mut input)? {
        match command {
            WorkerCommand::Synthesize {
                request_id, text, ..
            } => match text.as_str() {
                "cancel_wait" | "late_audio_after_cancel" => active = Some((request_id, text)),
                "malformed_once" if marker(&root, ".malformed-once")? => {
                    output.write_all(&[0_u8; 8])?;
                    output.flush()?;
                }
                "hang_once" if marker(&root, ".hang-once")? => {
                    active = Some((request_id, text));
                }
                _ => emit_healthy(&mut output, &request_id)?,
            },
            WorkerCommand::Cancel { request_id, .. } => {
                emit(&mut output, "cancel_ack", &request_id, 0, true, &[])?;
                if let Some((active_request, mode)) = active.take() {
                    match mode.as_str() {
                        "late_audio_after_cancel" => {
                            thread::sleep(Duration::from_millis(30));
                            emit(
                                &mut output,
                                "audio",
                                &active_request,
                                0,
                                false,
                                &[9, 0, 10, 0],
                            )?;
                            emit(&mut output, "cancelled", &active_request, 1, true, &[])?;
                        }
                        "cancel_wait" => {
                            emit(&mut output, "cancelled", &active_request, 0, true, &[])?;
                        }
                        // Deliberately withhold the terminal event so the
                        // provider has to enforce its response deadline.
                        "hang_once" => active = Some((active_request, mode)),
                        _ => unreachable!(),
                    }
                }
            }
            WorkerCommand::Shutdown { request_id } => {
                emit(&mut output, "shutdown_ack", &request_id, 0, true, &[])?;
                break;
            }
        }
    }
    Ok(())
}

fn main() {
    if run().is_err() {
        std::process::exit(2);
    }
}
