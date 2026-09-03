//! Native subtitle presentation boundary.
//!
//! A delivery receipt is intentionally stronger than a subtitle cue. Only a
//! sink that has committed a native surface may return `Ok(receipt)`.

use async_trait::async_trait;
use npc_runtime_core::{DeliveredSentence, TurnIdentity};
use tokio_util::sync::CancellationToken;

use crate::{
    SubtitleColorTreatment, SubtitleDirection, SubtitlePresentationProvenance,
    SubtitlePresentationReceiptSummary,
};

#[cfg(windows)]
use crate::simulation::{
    SubtitleContextProvenance, SubtitlePresentationContext, SubtitleTargetColorSpace,
};

#[cfg(windows)]
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

#[cfg(windows)]
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
    time::timeout,
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::FILETIME,
    System::{
        Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        Threading::{GetCurrentProcess, GetCurrentProcessId, GetProcessTimes},
    },
};

#[derive(Clone, Debug)]
pub struct SubtitlePresentationRequest<'a> {
    pub identity: &'a TurnIdentity,
    pub sentence: &'a DeliveredSentence,
    pub speaker: &'a str,
    pub locale: &'a str,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum SubtitlePresentationError {
    #[error("native subtitle presentation is unavailable")]
    Unavailable,
    #[error("native subtitle presentation was cancelled")]
    Cancelled,
    #[error("native subtitle presentation rejected the request: {0}")]
    Rejected(String),
    #[error("native subtitle surface did not commit")]
    NotCommitted,
}

#[async_trait]
pub trait SubtitlePresentationSink: Send + Sync {
    async fn present(
        &self,
        request: SubtitlePresentationRequest<'_>,
        cancellation: CancellationToken,
    ) -> Result<SubtitlePresentationReceiptSummary, SubtitlePresentationError>;
}

/// Lazily supervised client for the packaged GUI-subsystem presenter. The
/// child receives only inherited private pipes and a per-launch nonce; it has
/// no public command endpoint. A receipt is accepted only after the child
/// reports a committed native surface with exact turn/sentence evidence.
#[cfg(windows)]
pub struct NativeSubtitlePresentationSink {
    context: SubtitlePresentationContext,
    executable: PathBuf,
    state: Mutex<Option<NativePresenterState>>,
    next_presentation_id: AtomicU64,
}

#[cfg(windows)]
struct NativePresenterState {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    session_id: String,
    cancellation_generation: u64,
    next_sequence: u64,
}

#[cfg(windows)]
impl NativeSubtitlePresentationSink {
    pub fn packaged(
        context: SubtitlePresentationContext,
    ) -> Result<Self, SubtitlePresentationError> {
        let executable = std::env::current_exe()
            .map_err(|_| SubtitlePresentationError::Unavailable)?
            .with_file_name("npc-subtitle-presenter.exe");
        Ok(Self::at_path(context, executable))
    }

    fn at_path(context: SubtitlePresentationContext, executable: PathBuf) -> Self {
        Self {
            context,
            executable,
            state: Mutex::new(None),
            next_presentation_id: AtomicU64::new(1),
        }
    }

    async fn launch(
        &self,
        session_id: &str,
        cancellation_generation: u64,
    ) -> Result<NativePresenterState, SubtitlePresentationError> {
        launch_presenter(
            &self.executable,
            session_id,
            cancellation_generation,
            &self.context.renderer_authority,
        )
        .await
    }
}

#[cfg(windows)]
impl Drop for NativeSubtitlePresentationSink {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.try_lock() {
            if let Some(state) = state.as_mut() {
                let _ = state.child.start_kill();
            }
        }
    }
}

#[cfg(windows)]
#[async_trait]
impl SubtitlePresentationSink for NativeSubtitlePresentationSink {
    async fn present(
        &self,
        request: SubtitlePresentationRequest<'_>,
        cancellation: CancellationToken,
    ) -> Result<SubtitlePresentationReceiptSummary, SubtitlePresentationError> {
        if cancellation.is_cancelled() {
            return Err(SubtitlePresentationError::Cancelled);
        }
        let effective_context =
            effective_presentation_context(&self.context, context_is_fresh(&self.context)?);
        let mut guard = tokio::select! {
            () = cancellation.cancelled() => return Err(SubtitlePresentationError::Cancelled),
            guard = self.state.lock() => guard,
        };
        let identity = request.identity;
        let must_restart = guard.as_ref().is_some_and(|state| {
            state.session_id != identity.session_id
                || state.cancellation_generation != identity.cancellation_generation
        });
        if must_restart {
            if let Some(state) = guard.as_mut() {
                let _ = state.child.start_kill();
            }
            *guard = None;
        }
        if guard.is_none() {
            *guard = Some(tokio::select! {
                () = cancellation.cancelled() => return Err(SubtitlePresentationError::Cancelled),
                state = self.launch(&identity.session_id, identity.cancellation_generation) => state?,
            });
        }
        let state = guard.as_mut().expect("presenter state initialized");
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.saturating_add(1);
        let presentation_id = self.next_presentation_id.fetch_add(1, Ordering::Relaxed);
        let line = present_line(
            state,
            sequence,
            presentation_id,
            &effective_context,
            &request,
        )?;
        state
            .input
            .write_all(line.as_bytes())
            .await
            .map_err(|_| SubtitlePresentationError::Unavailable)?;
        state
            .input
            .write_all(b"\n")
            .await
            .map_err(|_| SubtitlePresentationError::Unavailable)?;
        state
            .input
            .flush()
            .await
            .map_err(|_| SubtitlePresentationError::Unavailable)?;
        let mut response = String::new();
        let read = tokio::select! {
            () = cancellation.cancelled() => {
                let _ = state.child.start_kill();
                return Err(SubtitlePresentationError::Cancelled);
            }
            read = timeout(Duration::from_secs(5), state.output.read_line(&mut response)) => read,
        };
        let bytes = read
            .map_err(|_| SubtitlePresentationError::Unavailable)?
            .map_err(|_| SubtitlePresentationError::Unavailable)?;
        if bytes == 0 || response.len() > 256 * 1024 {
            return Err(SubtitlePresentationError::Unavailable);
        }
        parse_receipt(
            response.trim_end_matches(['\r', '\n']),
            sequence,
            presentation_id,
            request.sentence.sentence_id,
            &effective_context,
        )
    }
}

#[cfg(windows)]
fn effective_presentation_context(
    context: &SubtitlePresentationContext,
    fresh: bool,
) -> SubtitlePresentationContext {
    if fresh {
        context.clone()
    } else {
        SubtitlePresentationContext::console_unavailable(context.renderer_authority.clone())
    }
}

#[cfg(windows)]
async fn launch_presenter(
    executable: &Path,
    session_id: &str,
    cancellation_generation: u64,
    authority: &npc_subtitle_engine::SubtitleRendererAuthorityV1,
) -> Result<NativePresenterState, SubtitlePresentationError> {
    if !executable.is_file() {
        return Err(SubtitlePresentationError::Unavailable);
    }
    let mut nonce = [0_u8; 32];
    nonce[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    nonce[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    let nonce_hex = hex_bytes(&nonce);
    // SAFETY: GetCurrentProcessId takes no pointers and cannot violate Rust memory safety.
    let process_id = unsafe { GetCurrentProcessId() };
    let creation_time = current_process_creation_time()?;
    let executable_name = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .ok_or(SubtitlePresentationError::Unavailable)?;
    let mut command = Command::new(executable);
    command
        .env("NPC_SUBTITLE_LAUNCH_NONCE", &nonce_hex)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .creation_flags(0x0800_0000);
    let mut child = command
        .spawn()
        .map_err(|_| SubtitlePresentationError::Unavailable)?;
    let mut input = child
        .stdin
        .take()
        .ok_or(SubtitlePresentationError::Unavailable)?;
    let output = child
        .stdout
        .take()
        .ok_or(SubtitlePresentationError::Unavailable)?;
    let mut output = BufReader::new(output);
    authority
        .validate()
        .map_err(|_| SubtitlePresentationError::Rejected("invalid renderer authority".into()))?;
    let style = &authority.style;
    let sources_json = serde_json::to_vec(&authority.sources)
        .map_err(|_| SubtitlePresentationError::Rejected("invalid renderer authority".into()))?;
    let hello = format!(
        "HELLO\t1\t1\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        deadline_qpc()?,
        cancellation_generation,
        nonce_hex,
        hex_text(session_id),
        process_id,
        creation_time,
        hex_text(&executable_name),
        authority.revision,
        authority.authority_sha256,
        hex_bytes(&sources_json),
        hex_text(&style.id),
        style.geometry.safe_margin_dp,
        authority.text_scale,
        style.typography.body_size_dp,
        style.typography.speaker_size_dp,
        style.typography.line_height,
        style.typography.max_body_lines,
        style.geometry.max_width_fraction,
        style.geometry.min_width_dp,
        style.geometry.max_width_dp,
        style.geometry.padding_x_dp,
        style.geometry.padding_y_dp,
        style.geometry.speaker_gap_dp,
        style.geometry.fallback_bottom_dp,
        style.geometry.corner_radius_dp,
        style.colors.body.r,
        style.colors.body.g,
        style.colors.body.b,
        style.colors.body.a,
        style.colors.speaker.r,
        style.colors.speaker.g,
        style.colors.speaker.b,
        style.colors.speaker.a,
        u8::from(style.effects.outline.enabled),
        style.effects.outline.width_dp,
        style.effects.outline.color.r,
        style.effects.outline.color.g,
        style.effects.outline.color.b,
        style.effects.outline.color.a,
        u8::from(style.effects.shadow.enabled),
        style.effects.shadow.offset_x_dp,
        style.effects.shadow.offset_y_dp,
        style.effects.shadow.blur_dp,
        style.effects.shadow.color.r,
        style.effects.shadow.color.g,
        style.effects.shadow.color.b,
        style.effects.shadow.color.a,
        u8::from(style.effects.backplate.enabled),
        style.effects.backplate.border_width_dp,
        style.effects.backplate.fill.r,
        style.effects.backplate.fill.g,
        style.effects.backplate.fill.b,
        style.effects.backplate.fill.a,
        style.effects.backplate.border.r,
        style.effects.backplate.border.g,
        style.effects.backplate.border.b,
        style.effects.backplate.border.a,
        authority.opacity,
    );
    input
        .write_all(hello.as_bytes())
        .await
        .map_err(|_| SubtitlePresentationError::Unavailable)?;
    input
        .flush()
        .await
        .map_err(|_| SubtitlePresentationError::Unavailable)?;
    let mut response = String::new();
    let bytes = timeout(Duration::from_secs(5), output.read_line(&mut response))
        .await
        .map_err(|_| SubtitlePresentationError::Unavailable)?
        .map_err(|_| SubtitlePresentationError::Unavailable)?;
    if bytes == 0 || response.trim_end_matches(['\r', '\n']) != "HELLO_OK\t1" {
        let _ = child.start_kill();
        return Err(SubtitlePresentationError::Rejected(
            "presenter authentication handshake failed".into(),
        ));
    }
    Ok(NativePresenterState {
        child,
        input,
        output,
        session_id: session_id.to_owned(),
        cancellation_generation,
        next_sequence: 2,
    })
}

#[cfg(windows)]
fn present_line(
    state: &NativePresenterState,
    sequence: u64,
    presentation_id: u64,
    context: &SubtitlePresentationContext,
    request: &SubtitlePresentationRequest<'_>,
) -> Result<String, SubtitlePresentationError> {
    let (provenance, target_pid, target_hwnd, target_executable) =
        match (context.provenance, context.target.as_ref()) {
            (SubtitleContextProvenance::TrustedNativeCapture, Some(target)) => (
                "native",
                target.process_id,
                target.window_handle,
                target.executable_name.as_str(),
            ),
            (SubtitleContextProvenance::ConsoleBottomCenterUnavailable, None) => {
                ("console", 0, 0, "")
            }
            _ => {
                return Err(SubtitlePresentationError::Rejected(
                    "invalid presentation context".into(),
                ))
            }
        };
    let color = match context.target_color_space {
        SubtitleTargetColorSpace::SdrSrgb => "sdr",
        SubtitleTargetColorSpace::SdrScRgb => "sdrscrgb",
        SubtitleTargetColorSpace::Hdr10Pq => "hdr10",
        SubtitleTargetColorSpace::HdrScRgb => "hdrscrgb",
        SubtitleTargetColorSpace::Unknown => "unknown",
    };
    let direction = match direction_for(&request.sentence.text) {
        SubtitleDirection::LeftToRight => "ltr",
        SubtitleDirection::RightToLeft => "rtl",
    };
    let exclusions = context
        .hud_exclusions_px
        .iter()
        .map(|rect| format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height))
        .collect::<Vec<_>>()
        .join(";");
    Ok(format!(
        "PRESENT\t1\t{sequence}\t{}\t{}\t{}\t{}\t{presentation_id}\t{provenance}\t{target_pid}\t{target_hwnd}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{direction}\t{color}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        deadline_qpc()?,
        state.cancellation_generation,
        hex_text(&request.identity.turn_id),
        request.sentence.sentence_id,
        hex_text(target_executable),
        context.viewport_px.x,
        context.viewport_px.y,
        context.viewport_px.width,
        context.viewport_px.height,
        context.dpi_x,
        context.dpi_y,
        context.sdr_white_level_nits,
        context.geometry_epoch.unwrap_or_default(),
        context.capture_sequence.unwrap_or_default(),
        context.graphics_generation.unwrap_or_default(),
        exclusions,
        hex_text(&request.sentence.text),
        hex_text(request.speaker),
        hex_text(request.locale),
        context.renderer_authority.revision,
        context.renderer_authority.authority_sha256,
    ))
}

#[cfg(windows)]
fn parse_receipt(
    line: &str,
    expected_sequence: u64,
    expected_presentation: u64,
    expected_sentence: u64,
    context: &SubtitlePresentationContext,
) -> Result<SubtitlePresentationReceiptSummary, SubtitlePresentationError> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.first() == Some(&"ERR") {
        return Err(SubtitlePresentationError::Rejected(
            fields
                .get(2)
                .copied()
                .unwrap_or("unknown presenter error")
                .to_owned(),
        ));
    }
    if fields.len() != 31 || fields[0] != "OK" {
        return Err(SubtitlePresentationError::Rejected(
            "malformed presenter receipt".into(),
        ));
    }
    let sequence = decimal::<u64>(fields[1])?;
    let receipt_id = decode_hex_text(fields[2])?;
    let sentence_id = decimal::<u64>(fields[3])?;
    let presentation_id = decimal::<u64>(fields[4])?;
    let provenance = match fields[5] {
        "native" => SubtitlePresentationProvenance::TrustedNativeCapture,
        "console" => SubtitlePresentationProvenance::ConsoleBottomCenterUnavailable,
        _ => {
            return Err(SubtitlePresentationError::Rejected(
                "invalid receipt provenance".into(),
            ))
        }
    };
    let expected_provenance = match context.provenance {
        SubtitleContextProvenance::TrustedNativeCapture => {
            SubtitlePresentationProvenance::TrustedNativeCapture
        }
        SubtitleContextProvenance::ConsoleBottomCenterUnavailable => {
            SubtitlePresentationProvenance::ConsoleBottomCenterUnavailable
        }
    };
    let target_geometry_epoch = decimal::<u64>(fields[6])?;
    let capture_sequence = decimal::<u64>(fields[7])?;
    let graphics_generation = decimal::<u64>(fields[8])?;
    if fields[9].len() != 16 || !fields[9].bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err(SubtitlePresentationError::Rejected(
            "invalid layer hash".into(),
        ));
    }
    let presented_qpc = decimal::<u64>(fields[10])?;
    let width_px = decimal::<u32>(fields[13])?;
    let height_px = decimal::<u32>(fields[14])?;
    let dpi_x = decimal::<u32>(fields[15])?;
    let dpi_y = decimal::<u32>(fields[16])?;
    let direction = match fields[17] {
        "ltr" => SubtitleDirection::LeftToRight,
        "rtl" => SubtitleDirection::RightToLeft,
        _ => {
            return Err(SubtitlePresentationError::Rejected(
                "invalid receipt direction".into(),
            ))
        }
    };
    let color_treatment = match fields[21] {
        "0" => SubtitleColorTreatment::SdrPremultipliedSourceOver,
        "1" => SubtitleColorTreatment::ScrgbLinearSourceOver,
        "2" => SubtitleColorTreatment::Hdr10ToneMappedSourceOver,
        "3" => SubtitleColorTreatment::WindowsCompositorSdrWhiteMapping,
        _ => {
            return Err(SubtitlePresentationError::Rejected(
                "invalid color treatment".into(),
            ))
        }
    };
    let expected_color_treatment = match context.target_color_space {
        SubtitleTargetColorSpace::SdrSrgb => SubtitleColorTreatment::SdrPremultipliedSourceOver,
        SubtitleTargetColorSpace::SdrScRgb
        | SubtitleTargetColorSpace::Hdr10Pq
        | SubtitleTargetColorSpace::HdrScRgb
        | SubtitleTargetColorSpace::Unknown => {
            SubtitleColorTreatment::WindowsCompositorSdrWhiteMapping
        }
    };
    if color_treatment != expected_color_treatment {
        return Err(SubtitlePresentationError::Rejected(
            "presenter reported a non-canonical color treatment".into(),
        ));
    }
    if sequence != expected_sequence
        || sentence_id != expected_sentence
        || presentation_id != expected_presentation
        || provenance != expected_provenance
        || receipt_id.is_empty()
        || presented_qpc == 0
        || width_px == 0
        || height_px == 0
        || dpi_x == 0
        || dpi_y == 0
        || fields[18] != "1"
        || fields[19] != "1"
        || fields[22] != "1"
        || decimal::<u64>(fields[23])? != context.renderer_authority.revision
        || fields[24] != context.renderer_authority.authority_sha256
        || decode_hex_text(fields[26])? != context.renderer_authority.style.id
        || decimal::<f32>(fields[27])? != context.renderer_authority.style.geometry.safe_margin_dp
        || decimal::<f32>(fields[28])? != context.renderer_authority.text_scale
        || (fields[29] == "1") != context.renderer_authority.style.effects.backplate.enabled
        || decimal::<f32>(fields[30])? != context.renderer_authority.opacity
    {
        return Err(SubtitlePresentationError::NotCommitted);
    }
    let sources_json = decode_hex_bytes(fields[25])?;
    let renderer_authority_sources = serde_json::from_slice(&sources_json)
        .map_err(|_| SubtitlePresentationError::Rejected("invalid authority sources".into()))?;
    if renderer_authority_sources != context.renderer_authority.sources {
        return Err(SubtitlePresentationError::Rejected(
            "presenter reported mismatched renderer authority sources".into(),
        ));
    }
    Ok(SubtitlePresentationReceiptSummary {
        receipt_id,
        sentence_id,
        provenance,
        presentation_id,
        target_geometry_epoch,
        capture_sequence,
        graphics_generation,
        layer_hash_hex: fields[9].to_owned(),
        presented_qpc_ticks: presented_qpc.to_string(),
        desktop_x_px: decimal::<i32>(fields[11])?,
        desktop_y_px: decimal::<i32>(fields[12])?,
        width_px,
        height_px,
        dpi_x,
        dpi_y,
        direction,
        bidi_shaping_applied: true,
        grapheme_clusters_preserved: true,
        used_bottom_center_fallback: fields[20] == "1",
        color_treatment,
        renderer_authority_revision: context.renderer_authority.revision,
        renderer_authority_sha256: context.renderer_authority.authority_sha256.clone(),
        renderer_authority_sources,
        renderer_style_id: context.renderer_authority.style.id.clone(),
        renderer_safe_area_dp: context.renderer_authority.style.geometry.safe_margin_dp,
        renderer_text_scale: context.renderer_authority.text_scale,
        renderer_backplate_enabled: context.renderer_authority.style.effects.backplate.enabled,
        renderer_opacity: context.renderer_authority.opacity,
        committed: true,
    })
}

#[cfg(windows)]
fn decimal<T>(value: &str) -> Result<T, SubtitlePresentationError>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| SubtitlePresentationError::Rejected("invalid numeric receipt field".into()))
}

#[cfg(windows)]
fn hex_text(value: &str) -> String {
    hex_bytes(value.as_bytes())
}

#[cfg(windows)]
fn hex_bytes(value: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(windows)]
fn decode_hex_text(value: &str) -> Result<String, SubtitlePresentationError> {
    if !value.len().is_multiple_of(2) || value.len() > 512 {
        return Err(SubtitlePresentationError::Rejected(
            "invalid receipt id".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = (pair[0] as char)
            .to_digit(16)
            .ok_or_else(|| SubtitlePresentationError::Rejected("invalid receipt id".into()))?;
        let low = (pair[1] as char)
            .to_digit(16)
            .ok_or_else(|| SubtitlePresentationError::Rejected("invalid receipt id".into()))?;
        bytes.push(((high << 4) | low) as u8);
    }
    String::from_utf8(bytes)
        .map_err(|_| SubtitlePresentationError::Rejected("invalid receipt id".into()))
}

#[cfg(windows)]
fn decode_hex_bytes(value: &str) -> Result<Vec<u8>, SubtitlePresentationError> {
    if !value.len().is_multiple_of(2) || value.len() > 32 * 1024 {
        return Err(SubtitlePresentationError::Rejected(
            "invalid authority source payload".into(),
        ));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).ok_or_else(|| {
                SubtitlePresentationError::Rejected("invalid authority source payload".into())
            })?;
            let low = (pair[1] as char).to_digit(16).ok_or_else(|| {
                SubtitlePresentationError::Rejected("invalid authority source payload".into())
            })?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}

#[cfg(windows)]
fn current_process_creation_time() -> Result<u64, SubtitlePresentationError> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: all FILETIME outputs are valid writable values for this process-only query.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        return Err(SubtitlePresentationError::Unavailable);
    }
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

#[cfg(windows)]
fn deadline_qpc() -> Result<u64, SubtitlePresentationError> {
    let mut now = 0_i64;
    let mut frequency = 0_i64;
    // SAFETY: both pointers reference initialized i64 storage for the duration of the calls.
    let valid = unsafe {
        QueryPerformanceCounter(&mut now) != 0 && QueryPerformanceFrequency(&mut frequency) != 0
    };
    if !valid || now <= 0 || frequency <= 0 {
        return Err(SubtitlePresentationError::Unavailable);
    }
    Ok((now as u64).saturating_add((frequency as u64).saturating_mul(10)))
}

#[cfg(windows)]
fn context_is_fresh(
    context: &SubtitlePresentationContext,
) -> Result<bool, SubtitlePresentationError> {
    if context.provenance == SubtitleContextProvenance::ConsoleBottomCenterUnavailable {
        return Ok(true);
    }
    let captured = context
        .capture_qpc
        .ok_or_else(|| SubtitlePresentationError::Rejected("missing_capture_qpc".into()))?;
    let attested = context
        .attested_at_qpc
        .ok_or_else(|| SubtitlePresentationError::Rejected("missing_attested_qpc".into()))?;
    let attested_frequency = context
        .qpc_frequency
        .ok_or_else(|| SubtitlePresentationError::Rejected("missing_qpc_frequency".into()))?;
    let mut now = 0_i64;
    let mut frequency = 0_i64;
    // SAFETY: both pointers reference initialized i64 storage for the duration of the calls.
    let valid = unsafe {
        QueryPerformanceCounter(&mut now) != 0 && QueryPerformanceFrequency(&mut frequency) != 0
    };
    if !valid || now <= 0 || frequency <= 0 {
        return Err(SubtitlePresentationError::Unavailable);
    }
    let now = now as u64;
    let frequency = frequency as u64;
    Ok(context_timestamps_are_fresh(
        captured,
        attested,
        attested_frequency,
        now,
        frequency,
    ))
}

#[cfg(windows)]
fn context_timestamps_are_fresh(
    captured: u64,
    attested: u64,
    attested_frequency: u64,
    now: u64,
    current_frequency: u64,
) -> bool {
    // Command 23 must observe an advancing frame within two seconds of its
    // attestation. The immutable turn context then remains usable for at most
    // thirty seconds; slower provider turns degrade to console subtitles
    // instead of presenting with stale target/device/geometry evidence.
    attested_frequency == current_frequency
        && captured <= attested
        && attested <= now
        && attested.saturating_sub(captured) <= current_frequency.saturating_mul(2)
        && now.saturating_sub(attested) <= current_frequency.saturating_mul(30)
}

/// Deterministic receipt surface for fixture-only turns. It never represents a
/// selected/native product route and is marked by a fixture receipt ID.
#[derive(Default)]
pub struct FixtureSubtitlePresentationSink;

#[async_trait]
impl SubtitlePresentationSink for FixtureSubtitlePresentationSink {
    async fn present(
        &self,
        request: SubtitlePresentationRequest<'_>,
        cancellation: CancellationToken,
    ) -> Result<SubtitlePresentationReceiptSummary, SubtitlePresentationError> {
        if cancellation.is_cancelled() {
            return Err(SubtitlePresentationError::Cancelled);
        }
        let sentence_id = request.sentence.sentence_id;
        let presentation_id = sentence_id.saturating_add(1);
        let authority = npc_subtitle_engine::bundled_default_renderer_authority()
            .map_err(|_| SubtitlePresentationError::Unavailable)?;
        Ok(SubtitlePresentationReceiptSummary {
            receipt_id: format!(
                "fixture-subtitle-{}-{}-{}",
                request.identity.turn_id, request.identity.cancellation_generation, sentence_id
            ),
            sentence_id,
            provenance: SubtitlePresentationProvenance::DeterministicFixture,
            presentation_id,
            target_geometry_epoch: 1,
            capture_sequence: 1,
            graphics_generation: 1,
            layer_hash_hex: format!("{:016x}", presentation_id ^ sentence_id.rotate_left(17)),
            presented_qpc_ticks: "1".into(),
            desktop_x_px: 0,
            desktop_y_px: 0,
            width_px: 1,
            height_px: 1,
            dpi_x: 96,
            dpi_y: 96,
            direction: direction_for(request.sentence.text.as_str()),
            bidi_shaping_applied: true,
            grapheme_clusters_preserved: true,
            used_bottom_center_fallback: true,
            color_treatment: SubtitleColorTreatment::SdrPremultipliedSourceOver,
            renderer_authority_revision: authority.revision,
            renderer_authority_sha256: authority.authority_sha256.clone(),
            renderer_authority_sources: authority.sources.clone(),
            renderer_style_id: authority.style.id.clone(),
            renderer_safe_area_dp: authority.style.geometry.safe_margin_dp,
            renderer_text_scale: authority.text_scale,
            renderer_backplate_enabled: authority.style.effects.backplate.enabled,
            renderer_opacity: authority.opacity,
            committed: true,
        })
    }
}

fn direction_for(text: &str) -> SubtitleDirection {
    for character in text.chars() {
        match character as u32 {
            0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF => {
                return SubtitleDirection::RightToLeft;
            }
            0x0041..=0x052F | 0x0900..=0xD7AF => return SubtitleDirection::LeftToRight,
            _ => {}
        }
    }
    SubtitleDirection::LeftToRight
}

#[cfg(test)]
mod tests {
    use super::*;
    use npc_runtime_core::{DeliveredSentence, DeliveryMode, TurnIdentity};

    #[tokio::test]
    async fn fixture_receipt_is_committed_and_rtl_attested() {
        let sink = FixtureSubtitlePresentationSink;
        let identity = TurnIdentity {
            session_id: "session".into(),
            turn_id: "turn".into(),
            cancellation_generation: 4,
        };
        let sentence = DeliveredSentence {
            sentence_id: 8,
            text_start_bytes: 0,
            text_end_bytes: 10,
            text: "مرحبا".into(),
            delivery: DeliveryMode::Subtitle,
            audible_frames: 0,
            duration: std::time::Duration::ZERO,
        };
        let receipt = sink
            .present(
                SubtitlePresentationRequest {
                    identity: &identity,
                    sentence: &sentence,
                    speaker: "Mara",
                    locale: "ar-SA",
                },
                CancellationToken::new(),
            )
            .await
            .expect("fixture commit");
        assert!(receipt.committed);
        assert_eq!(receipt.sentence_id, 8);
        assert_eq!(receipt.direction, SubtitleDirection::RightToLeft);
        assert!(receipt.bidi_shaping_applied);
    }

    #[cfg(windows)]
    fn console_context() -> SubtitlePresentationContext {
        SubtitlePresentationContext {
            schema_version: 1,
            provenance: SubtitleContextProvenance::ConsoleBottomCenterUnavailable,
            target: None,
            viewport_px: crate::simulation::SubtitleRectPx {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            dpi_x: 0,
            dpi_y: 0,
            target_color_space: SubtitleTargetColorSpace::Unknown,
            sdr_white_level_nits: 0.0,
            capture_device_generation: None,
            geometry_epoch: None,
            capture_sequence: None,
            capture_qpc: None,
            graphics_generation: None,
            attested_at_qpc: None,
            qpc_frequency: None,
            attestation_id: None,
            hud_exclusions_px: Vec::new(),
            renderer_authority: npc_subtitle_engine::bundled_default_renderer_authority()
                .expect("bundled renderer authority"),
        }
    }

    #[cfg(windows)]
    fn committed_console_line(
        color_treatment: u8,
        context: &SubtitlePresentationContext,
    ) -> String {
        let authority = &context.renderer_authority;
        let sources = serde_json::to_vec(&authority.sources).expect("serialize sources");
        format!(
            "OK\t2\t7375627469746c652d31\t7\t11\tconsole\t0\t0\t0\t0123456789abcdef\t99\t100\t200\t600\t120\t144\t144\trtl\t1\t1\t1\t{color_treatment}\t1\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            authority.revision,
            authority.authority_sha256,
            hex_bytes(&sources),
            hex_text(&authority.style.id),
            authority.style.geometry.safe_margin_dp,
            authority.text_scale,
            u8::from(authority.style.effects.backplate.enabled),
            authority.opacity,
        )
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn cancelled_request_never_launches_an_unavailable_presenter() {
        let sink = NativeSubtitlePresentationSink::at_path(
            console_context(),
            PathBuf::from(r"Z:\definitely-missing\npc-subtitle-presenter.exe"),
        );
        let identity = TurnIdentity {
            session_id: "session".into(),
            turn_id: "turn".into(),
            cancellation_generation: 3,
        };
        let sentence = DeliveredSentence {
            sentence_id: 1,
            text_start_bytes: 0,
            text_end_bytes: 5,
            text: "Hello".into(),
            delivery: DeliveryMode::Subtitle,
            audible_frames: 0,
            duration: std::time::Duration::ZERO,
        };
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            sink.present(
                SubtitlePresentationRequest {
                    identity: &identity,
                    sentence: &sentence,
                    speaker: "Mara",
                    locale: "en-US",
                },
                cancellation,
            )
            .await,
            Err(SubtitlePresentationError::Cancelled)
        );
    }

    #[cfg(windows)]
    #[test]
    fn strict_console_receipt_keeps_unavailable_provenance_and_zero_capture_ids() {
        let context = console_context();
        let receipt = parse_receipt(&committed_console_line(3, &context), 2, 11, 7, &context)
            .expect("strict committed receipt");
        assert_eq!(
            receipt.provenance,
            SubtitlePresentationProvenance::ConsoleBottomCenterUnavailable
        );
        assert_eq!(receipt.target_geometry_epoch, 0);
        assert_eq!(receipt.capture_sequence, 0);
        assert_eq!(receipt.direction, SubtitleDirection::RightToLeft);
        assert!(receipt.committed);
    }

    #[cfg(windows)]
    #[test]
    fn pre_v1_tone_mapped_receipt_cannot_claim_current_hdr_delivery() {
        let context = console_context();
        assert_eq!(
            parse_receipt(&committed_console_line(2, &context), 2, 11, 7, &context,),
            Err(SubtitlePresentationError::Rejected(
                "presenter reported a non-canonical color treatment".into()
            ))
        );
    }

    #[cfg(windows)]
    #[test]
    fn receipt_rejects_renderer_authority_mismatch() {
        let context = console_context();
        let line = committed_console_line(3, &context).replace(
            &context.renderer_authority.authority_sha256,
            &"0".repeat(64),
        );
        assert!(matches!(
            parse_receipt(&line, 2, 11, 7, &context),
            Err(SubtitlePresentationError::NotCommitted)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn future_trusted_capture_context_is_rejected_as_stale() {
        let mut context = console_context();
        context.provenance = SubtitleContextProvenance::TrustedNativeCapture;
        context.capture_qpc = Some(u64::MAX - 1);
        context.attested_at_qpc = Some(u64::MAX);
        context.qpc_frequency = Some(10_000_000);
        assert_eq!(context_is_fresh(&context), Ok(false));
    }

    #[cfg(windows)]
    #[test]
    fn command_23_attestation_freshness_has_exact_closed_bounds() {
        let frequency = 10_000_000;
        let attested = 100 * frequency;
        assert!(context_timestamps_are_fresh(
            attested - 2 * frequency,
            attested,
            frequency,
            attested + 30 * frequency,
            frequency,
        ));
        assert!(!context_timestamps_are_fresh(
            attested - 2 * frequency - 1,
            attested,
            frequency,
            attested,
            frequency,
        ));
        assert!(!context_timestamps_are_fresh(
            attested,
            attested,
            frequency,
            attested + 30 * frequency + 1,
            frequency,
        ));
        assert!(!context_timestamps_are_fresh(
            attested,
            attested,
            frequency + 1,
            attested,
            frequency,
        ));
    }

    #[cfg(windows)]
    #[test]
    fn stale_trusted_context_degrades_only_to_explicit_console_provenance() {
        let mut trusted = console_context();
        trusted.provenance = SubtitleContextProvenance::TrustedNativeCapture;
        trusted.target = Some(crate::simulation::SubtitleTargetIdentity {
            process_id: 42,
            window_handle: 99,
            executable_name: "game.exe".into(),
        });
        trusted.capture_device_generation = Some(3);
        trusted.geometry_epoch = Some(4);
        trusted.capture_sequence = Some(5);
        trusted.capture_qpc = Some(6);
        trusted.graphics_generation = Some(3);
        trusted.attested_at_qpc = Some(7);
        trusted.qpc_frequency = Some(10_000_000);
        trusted.attestation_id = Some(8);
        let fallback = effective_presentation_context(&trusted, false);
        assert_eq!(
            fallback.provenance,
            SubtitleContextProvenance::ConsoleBottomCenterUnavailable
        );
        assert!(fallback.target.is_none());
        assert_eq!(fallback.viewport_px.width, 0);
        assert!(fallback.capture_device_generation.is_none());
        assert!(fallback.attestation_id.is_none());
    }
}
