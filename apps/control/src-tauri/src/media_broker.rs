use crate::domain::{MediaBrokerDiagnostics, MediaBrokerHealthSnapshot, RuntimeConnectionState};
use crate::sidecar_supervisor::RuntimeSupervisor;
use prost::{Enumeration, Message};
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
#[cfg(not(windows))]
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};

const BROKER_FILE_NAME: &str = if cfg!(windows) {
    "npc-media-broker.exe"
} else {
    "npc-media-broker"
};
const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME_BYTES: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const FAILURE_WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILURES: usize = 3;
const AUDIO_OUTPUT_SELECTION_FILE_NAME: &str = "audio-output-selection-v1.json";
const AUDIO_INPUT_SELECTION_FILE_NAME: &str = "audio-input-selection-v1.json";
pub const MAX_PLAYBACK_LEASES_PER_TURN: usize = 16;
const MAX_VISUAL_SPEECH_CUES: usize = 128;
pub const MIN_INPUT_REHEARSAL_DURATION_MS: u32 = 250;
pub const MAX_INPUT_REHEARSAL_DURATION_MS: u32 = 10_000;
#[cfg(debug_assertions)]
const DEBUG_SYNTHETIC_TARGET_BASENAME: &str = "interactive-npcs-synthetic-target.exe";
#[cfg(debug_assertions)]
const DEBUG_SYNTHETIC_METADATA_MAX_BYTES: u64 = 32 * 1024;
#[cfg(debug_assertions)]
const DEBUG_SYNTHETIC_METADATA_FILE_NAME: &str = "debug-synthetic-replay-target.json";
#[cfg(debug_assertions)]
const DEBUG_SYNTHETIC_PORTRAIT_SHA256: &str =
    "0ae10605199bb831c34a3be29c31a06f8a1d5a7e5cd07b8a1456866f2bb7dc2d";
#[cfg(debug_assertions)]
const DEBUG_SYNTHETIC_VISUAL_SOURCE: &str =
    "embedded-original-generated-photorealistic-portrait-v1";
#[cfg(debug_assertions)]
const DEBUG_SYNTHETIC_MOVING_VISUAL_SOURCE: &str =
    "verified-sibling-project-owned-mara-camera-sequence-v1";
#[cfg(debug_assertions)]
const DEBUG_SYNTHETIC_SOURCE_SEQUENCE_SHA256: &str =
    "22022d1564ba8f74137fe8b6813dd1d22dc47b00fa013632c6f6bbbc60bb265d";

#[derive(Clone, PartialEq, Message)]
struct BrokerEnvelope {
    #[prost(uint32, tag = "1")]
    version: u32,
    #[prost(bytes = "vec", tag = "2")]
    launch_nonce: Vec<u8>,
    #[prost(string, tag = "3")]
    session_id: String,
    #[prost(uint64, tag = "4")]
    sequence: u64,
    #[prost(uint64, tag = "5")]
    deadline_qpc: u64,
    #[prost(uint64, tag = "6")]
    cancellation_generation: u64,
    #[prost(enumeration = "BrokerCommand", tag = "7")]
    command: i32,
    #[prost(bytes = "vec", tag = "8")]
    payload: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct BrokerResponse {
    #[prost(uint32, tag = "1")]
    version: u32,
    #[prost(uint64, tag = "2")]
    response_to_sequence: u64,
    #[prost(enumeration = "BrokerStatus", tag = "3")]
    status: i32,
    #[prost(uint64, tag = "4")]
    cancellation_generation: u64,
    #[prost(bytes = "vec", tag = "5")]
    payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
enum BrokerCommand {
    Unspecified = 0,
    Health = 1,
    SelectTarget = 2,
    ClearTarget = 3,
    SubmitOcclusion = 6,
    SubmitPatch = 7,
    Diagnostics = 9,
    Shutdown = 10,
    CaptureEvidence = 11,
    AllocatePlaybackStream = 12,
    CancelPlayback = 13,
    AllocateVisualSource = 14,
    ReleaseVisualSource = 15,
    AllocateIdentityFrame = 16,
    ReleaseIdentityFrame = 17,
    EnumerateAudioOutputs = 18,
    SelectAudioOutput = 19,
    SelectedAudioOutput = 20,
    AllocateIdentityReferenceImport = 21,
    ReleaseIdentityReferenceImport = 22,
    TrustedSubtitlePresentationContext = 23,
    EnumerateAudioInputs = 24,
    SelectAudioInput = 25,
    SelectedAudioInput = 26,
    AllocateAudioInputRehearsal = 27,
    CancelAudioInputRehearsal = 28,
    QueryVisualAudioEnvelope = 29,
    QueryPttActivationState = 30,
    ManualActorPicker = 31,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
enum ManualActorPickerActionPayload {
    Unspecified = 0,
    Begin = 1,
    Poll = 2,
    Cancel = 3,
}

#[derive(Clone, PartialEq, Message)]
struct ManualActorCandidatePayload {
    #[prost(uint64, tag = "1")]
    actor_id: u64,
    #[prost(uint64, tag = "2")]
    track_id: u64,
    #[prost(uint64, tag = "3")]
    track_epoch: u64,
    #[prost(double, tag = "4")]
    left: f64,
    #[prost(double, tag = "5")]
    top: f64,
    #[prost(double, tag = "6")]
    right: f64,
    #[prost(double, tag = "7")]
    bottom: f64,
}

#[derive(Clone, PartialEq, Message)]
struct ManualActorPickerCommandPayload {
    #[prost(enumeration = "ManualActorPickerActionPayload", tag = "1")]
    action: i32,
    #[prost(string, tag = "2")]
    request_id: String,
    #[prost(uint64, tag = "3")]
    source_device_generation: u64,
    #[prost(uint64, tag = "4")]
    source_geometry_epoch: u64,
    #[prost(uint64, tag = "5")]
    source_frame_sequence: u64,
    #[prost(uint64, tag = "6")]
    source_frame_qpc: u64,
    #[prost(uint32, tag = "7")]
    timeout_ms: u32,
    #[prost(message, repeated, tag = "8")]
    candidates: Vec<ManualActorCandidatePayload>,
}

#[derive(Clone, PartialEq, Message)]
struct ManualActorPickerReceiptPayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(string, tag = "2")]
    request_id: String,
    #[prost(uint32, tag = "3")]
    status: u32,
    #[prost(fixed64, tag = "4")]
    receipt_nonce_high: u64,
    #[prost(fixed64, tag = "5")]
    receipt_nonce_low: u64,
    #[prost(string, tag = "6")]
    capture_session_id: String,
    #[prost(uint64, tag = "7")]
    cancellation_generation: u64,
    #[prost(uint32, tag = "8")]
    selected_process_id: u32,
    #[prost(fixed64, tag = "9")]
    selected_window_handle: u64,
    #[prost(string, tag = "10")]
    selected_executable_name: String,
    #[prost(uint64, tag = "11")]
    source_device_generation: u64,
    #[prost(uint64, tag = "12")]
    source_geometry_epoch: u64,
    #[prost(uint64, tag = "13")]
    source_frame_sequence: u64,
    #[prost(uint64, tag = "14")]
    source_frame_qpc: u64,
    #[prost(uint64, tag = "15")]
    selected_actor_id: u64,
    #[prost(uint64, tag = "16")]
    selected_track_id: u64,
    #[prost(uint64, tag = "17")]
    selected_track_epoch: u64,
    #[prost(uint32, tag = "18")]
    candidate_count: u32,
    #[prost(string, tag = "19")]
    candidate_set_sha256: String,
    #[prost(uint64, tag = "20")]
    began_qpc: u64,
    #[prost(uint64, tag = "21")]
    clicked_qpc: u64,
    #[prost(uint64, tag = "22")]
    attested_at_qpc: u64,
    #[prost(uint64, tag = "23")]
    qpc_frequency: u64,
    #[prost(uint32, tag = "24")]
    pointer_kind: u32,
    #[prost(bool, tag = "25")]
    frozen_wgc_frame_verified: bool,
    #[prost(bool, tag = "26")]
    overlay_capture_excluded: bool,
    #[prost(bool, tag = "27")]
    overlay_nonactivating: bool,
    #[prost(bool, tag = "28")]
    single_hardware_pointer_click: bool,
    #[prost(bool, tag = "29")]
    pixels_withheld_from_webview: bool,
    #[prost(bool, tag = "30")]
    coordinates_withheld_from_webview: bool,
}

#[derive(Clone, PartialEq, Message)]
struct PttActivationStatePayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(uint32, tag = "2")]
    virtual_key: u32,
    #[prost(uint32, tag = "3")]
    state: u32,
    #[prost(uint64, tag = "4")]
    transition_sequence: u64,
    #[prost(uint64, tag = "5")]
    transition_qpc: u64,
    #[prost(uint64, tag = "6")]
    release_transition_sequence: u64,
    #[prost(uint64, tag = "7")]
    released_qpc: u64,
}

#[derive(Clone, PartialEq, Message)]
struct QueryVisualAudioEnvelopePayload {
    #[prost(string, tag = "1")]
    session_id: String,
    #[prost(string, tag = "2")]
    turn_id: String,
    #[prost(uint64, tag = "3")]
    generation: u64,
    #[prost(string, tag = "4")]
    stream_id: String,
    #[prost(string, tag = "5")]
    segment_id: String,
}

#[derive(Clone, PartialEq, Message)]
struct VisualSpeechCuePayload {
    #[prost(uint64, tag = "1")]
    start_sample: u64,
    #[prost(uint64, tag = "2")]
    duration_samples: u64,
    #[prost(uint32, tag = "3")]
    canonical_viseme: u32,
    #[prost(uint32, tag = "4")]
    strength_q15: u32,
}

#[derive(Clone, PartialEq, Message)]
struct VisualAudioEnvelopePayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(string, tag = "2")]
    session_id: String,
    #[prost(string, tag = "3")]
    turn_id: String,
    #[prost(uint64, tag = "4")]
    generation: u64,
    #[prost(string, tag = "5")]
    stream_id: String,
    #[prost(string, tag = "6")]
    segment_id: String,
    #[prost(uint64, tag = "7")]
    source_sample_start: u64,
    #[prost(uint32, tag = "8")]
    source_sample_count: u32,
    #[prost(uint32, tag = "9")]
    sample_rate: u32,
    #[prost(uint32, tag = "10")]
    channels: u32,
    #[prost(uint64, tag = "11")]
    device_write_qpc: u64,
    #[prost(uint64, tag = "12")]
    qpc_frequency: u64,
    #[prost(uint64, tag = "13")]
    source_frames: u64,
    #[prost(uint64, tag = "14")]
    device_frames: u64,
    #[prost(uint32, repeated, tag = "15")]
    mono_rms_q15: Vec<u32>,
    #[prost(uint32, repeated, tag = "16")]
    mono_peak_q15: Vec<u32>,
    #[prost(bool, tag = "17")]
    active: bool,
    #[prost(bool, tag = "18")]
    draining: bool,
    #[prost(bool, tag = "19")]
    cancelled: bool,
    #[prost(message, repeated, tag = "20")]
    visual_speech_cues: Vec<VisualSpeechCuePayload>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
enum AudioOutputSelectionModePayload {
    Unspecified = 0,
    SystemDefault = 1,
    EndpointId = 2,
}

#[derive(Clone, PartialEq, Message)]
struct SelectAudioOutputPayload {
    #[prost(enumeration = "AudioOutputSelectionModePayload", tag = "1")]
    mode: i32,
    #[prost(string, tag = "2")]
    endpoint_id: String,
}

#[derive(Clone, PartialEq, Message)]
struct AudioOutputEndpointPayload {
    #[prost(string, tag = "1")]
    endpoint_id: String,
    #[prost(string, tag = "2")]
    friendly_name: String,
    #[prost(uint32, tag = "3")]
    state: u32,
    #[prost(bool, tag = "4")]
    system_default: bool,
    #[prost(uint64, tag = "5")]
    generation: u64,
}

#[derive(Clone, PartialEq, Message)]
struct AudioOutputSnapshotPayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(uint64, tag = "2")]
    catalog_generation: u64,
    #[prost(message, repeated, tag = "3")]
    endpoints: Vec<AudioOutputEndpointPayload>,
}

#[derive(Clone, PartialEq, Message)]
struct SelectedAudioOutputPayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(enumeration = "AudioOutputSelectionModePayload", tag = "2")]
    mode: i32,
    #[prost(string, tag = "3")]
    requested_endpoint_id: String,
    #[prost(message, optional, tag = "4")]
    resolved: Option<AudioOutputEndpointPayload>,
}

#[derive(Clone, PartialEq, Message)]
struct AudioInputEndpointPayload {
    #[prost(string, tag = "1")]
    endpoint_id: String,
    #[prost(string, tag = "2")]
    friendly_name: String,
    #[prost(uint32, tag = "3")]
    state: u32,
    #[prost(bool, tag = "4")]
    system_default: bool,
    #[prost(uint64, tag = "5")]
    generation: u64,
}

#[derive(Clone, PartialEq, Message)]
struct AudioInputSnapshotPayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(uint64, tag = "2")]
    catalog_generation: u64,
    #[prost(message, repeated, tag = "3")]
    endpoints: Vec<AudioInputEndpointPayload>,
}

#[derive(Clone, PartialEq, Message)]
struct SelectedAudioInputPayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(enumeration = "AudioOutputSelectionModePayload", tag = "2")]
    mode: i32,
    #[prost(string, tag = "3")]
    requested_endpoint_id: String,
    #[prost(message, optional, tag = "4")]
    resolved: Option<AudioInputEndpointPayload>,
}

#[derive(Clone, PartialEq, Message)]
struct AllocateAudioInputRehearsalPayload {
    #[prost(string, tag = "1")]
    session_id: String,
    #[prost(string, tag = "2")]
    turn_id: String,
    #[prost(uint64, tag = "3")]
    generation: u64,
    #[prost(uint32, tag = "4")]
    duration_ms: u32,
    #[prost(uint32, tag = "5")]
    sample_rate: u32,
    #[prost(uint32, tag = "6")]
    channels: u32,
    #[prost(uint64, tag = "7")]
    max_frames: u64,
    #[prost(uint32, tag = "8")]
    expected_producer_process_id: u32,
    #[prost(enumeration = "InputActivationSourcePayload", tag = "9")]
    activation_source: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
enum InputActivationSourcePayload {
    Unspecified = 0,
    ExplicitRehearsal = 1,
    PushToTalk = 2,
}

#[derive(Clone, PartialEq, Message)]
struct CancelAudioInputRehearsalPayload {
    #[prost(string, tag = "1")]
    stream_id: String,
    #[prost(uint64, tag = "2")]
    generation: u64,
}

#[derive(Clone, PartialEq, Message)]
struct AudioInputRehearsalLeasePayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(string, tag = "2")]
    stream_id: String,
    #[prost(string, tag = "3")]
    producer_endpoint: String,
    #[prost(bytes = "vec", tag = "4")]
    one_time_token: Vec<u8>,
    #[prost(string, tag = "5")]
    session_id: String,
    #[prost(string, tag = "6")]
    turn_id: String,
    #[prost(uint64, tag = "7")]
    generation: u64,
    #[prost(uint32, tag = "8")]
    duration_ms: u32,
    #[prost(uint32, tag = "9")]
    sample_rate: u32,
    #[prost(uint32, tag = "10")]
    channels: u32,
    #[prost(uint64, tag = "11")]
    max_frames: u64,
    #[prost(uint32, tag = "12")]
    max_chunk_bytes: u32,
    #[prost(uint64, tag = "13")]
    expires_qpc: u64,
    #[prost(uint64, tag = "14")]
    qpc_frequency: u64,
    #[prost(enumeration = "AudioOutputSelectionModePayload", tag = "15")]
    input_selection_mode: i32,
    #[prost(string, tag = "16")]
    input_endpoint_id: String,
    #[prost(uint64, tag = "17")]
    input_endpoint_generation: u64,
    #[prost(enumeration = "InputActivationSourcePayload", tag = "18")]
    activation_source: i32,
    #[prost(uint32, tag = "19")]
    ptt_virtual_key: u32,
    #[prost(uint64, tag = "20")]
    ptt_press_transition_sequence: u64,
    #[prost(uint64, tag = "21")]
    ptt_pressed_qpc: u64,
}

#[derive(Clone, PartialEq, Message)]
struct TrustedSubtitlePresentationContextPayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(uint32, tag = "2")]
    selected_process_id: u32,
    #[prost(fixed64, tag = "3")]
    selected_window: u64,
    #[prost(uint64, tag = "4")]
    capture_device_generation: u64,
    #[prost(uint64, tag = "5")]
    geometry_epoch: u64,
    #[prost(uint64, tag = "6")]
    source_frame_sequence: u64,
    #[prost(uint64, tag = "7")]
    source_frame_qpc: u64,
    #[prost(sint32, tag = "8")]
    window_left_px: i32,
    #[prost(sint32, tag = "9")]
    window_top_px: i32,
    #[prost(sint32, tag = "10")]
    window_right_px: i32,
    #[prost(sint32, tag = "11")]
    window_bottom_px: i32,
    #[prost(sint32, tag = "12")]
    client_left_px: i32,
    #[prost(sint32, tag = "13")]
    client_top_px: i32,
    #[prost(sint32, tag = "14")]
    client_right_px: i32,
    #[prost(sint32, tag = "15")]
    client_bottom_px: i32,
    #[prost(uint32, tag = "16")]
    captured_width_px: u32,
    #[prost(uint32, tag = "17")]
    captured_height_px: u32,
    #[prost(string, tag = "18")]
    monitor_id: String,
    #[prost(sint32, tag = "19")]
    monitor_left_px: i32,
    #[prost(sint32, tag = "20")]
    monitor_top_px: i32,
    #[prost(sint32, tag = "21")]
    monitor_right_px: i32,
    #[prost(sint32, tag = "22")]
    monitor_bottom_px: i32,
    #[prost(sint32, tag = "23")]
    work_left_px: i32,
    #[prost(sint32, tag = "24")]
    work_top_px: i32,
    #[prost(sint32, tag = "25")]
    work_right_px: i32,
    #[prost(sint32, tag = "26")]
    work_bottom_px: i32,
    #[prost(bool, tag = "27")]
    dpi_available: bool,
    #[prost(uint32, tag = "28")]
    dpi_x: u32,
    #[prost(uint32, tag = "29")]
    dpi_y: u32,
    #[prost(bool, tag = "30")]
    hdr_evidence_available: bool,
    #[prost(bool, tag = "31")]
    hdr_supported: bool,
    #[prost(bool, tag = "32")]
    hdr_user_enabled: bool,
    #[prost(bool, tag = "33")]
    hdr_active: bool,
    #[prost(bool, tag = "34")]
    advanced_color_active: bool,
    #[prost(uint32, tag = "35")]
    active_color_mode: u32,
    #[prost(bool, tag = "36")]
    color_encoding_available: bool,
    #[prost(uint32, tag = "37")]
    color_encoding: u32,
    #[prost(uint32, tag = "38")]
    bits_per_color_channel: u32,
    #[prost(bool, tag = "39")]
    sdr_white_level_available: bool,
    #[prost(double, tag = "40")]
    sdr_white_level_nits: f64,
    #[prost(uint32, tag = "41")]
    capture_backend: u32,
    #[prost(uint32, tag = "42")]
    capture_scope: u32,
    #[prost(bool, tag = "43")]
    overlay_capture_excluded: bool,
    #[prost(bool, tag = "44")]
    overlay_visuals_allowed: bool,
    #[prost(string, tag = "45")]
    selected_executable_name: String,
    #[prost(bool, tag = "46")]
    target_color_space_available: bool,
    #[prost(uint32, tag = "47")]
    target_color_space: u32,
    #[prost(uint64, tag = "48")]
    attested_at_qpc: u64,
    #[prost(uint64, tag = "49")]
    qpc_frequency: u64,
    #[prost(uint64, tag = "50")]
    attestation_id: u64,
}

#[derive(Clone, PartialEq, Message)]
struct AllocateIdentityFramePayload {
    #[prost(uint32, tag = "1")]
    worker_process_id: u32,
    #[prost(fixed64, tag = "2")]
    worker_process_creation_time: u64,
    #[prost(string, tag = "3")]
    worker_executable_name: String,
    #[prost(uint32, tag = "4")]
    crop_left: u32,
    #[prost(uint32, tag = "5")]
    crop_top: u32,
    #[prost(uint32, tag = "6")]
    crop_right: u32,
    #[prost(uint32, tag = "7")]
    crop_bottom: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
enum IdentityReferenceSourceClassPayload {
    Unspecified = 0,
    UserPrivate = 1,
    OriginalSynthetic = 2,
}

#[derive(Clone, PartialEq, Message)]
struct AllocateIdentityReferenceImportPayload {
    #[prost(uint32, tag = "1")]
    worker_process_id: u32,
    #[prost(fixed64, tag = "2")]
    worker_process_creation_time: u64,
    #[prost(string, tag = "3")]
    worker_executable_name: String,
    #[prost(string, tag = "4")]
    picker_consent_token: String,
    #[prost(string, tag = "5")]
    game_profile_id: String,
    #[prost(string, tag = "6")]
    character_id: String,
    #[prost(string, tag = "7")]
    subject_id: String,
    #[prost(string, tag = "8")]
    reference_id: String,
    #[prost(string, tag = "9")]
    subject_display_name: String,
    #[prost(enumeration = "IdentityReferenceSourceClassPayload", tag = "10")]
    source_class: i32,
    #[prost(string, tag = "11")]
    owner_user_id: String,
    #[prost(string, tag = "12")]
    original_work_license: String,
    #[prost(bool, tag = "13")]
    explicit_user_consent: bool,
    #[prost(bool, tag = "14")]
    local_only: bool,
    #[prost(uint64, tag = "15")]
    imported_at_unix_ms: u64,
}

#[derive(Clone, PartialEq, Message)]
struct ReleaseIdentityReferenceImportPayload {
    #[prost(uint32, tag = "1")]
    worker_process_id: u32,
    #[prost(string, tag = "2")]
    lease_id: String,
    #[prost(string, tag = "3")]
    lease_nonce: String,
}

#[derive(Clone, PartialEq, Message)]
struct IdentityReferenceImportLeasePayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(uint32, tag = "2")]
    worker_process_id: u32,
    #[prost(fixed64, tag = "3")]
    worker_process_creation_time: u64,
    #[prost(string, tag = "4")]
    worker_executable_name: String,
    #[prost(string, tag = "5")]
    lease_id: String,
    #[prost(string, tag = "6")]
    shared_memory_name: String,
    #[prost(string, tag = "7")]
    lease_nonce: String,
    #[prost(uint64, tag = "8")]
    byte_length: u64,
    #[prost(uint32, tag = "9")]
    width: u32,
    #[prost(uint32, tag = "10")]
    height: u32,
    #[prost(uint32, tag = "11")]
    stride_bytes: u32,
    #[prost(string, tag = "12")]
    pixel_format: String,
    #[prost(string, tag = "13")]
    content_sha256: String,
    #[prost(string, tag = "14")]
    source_asset_sha256: String,
    #[prost(string, tag = "15")]
    source_media_type: String,
    #[prost(uint64, tag = "16")]
    expires_qpc: u64,
    #[prost(uint64, tag = "17")]
    qpc_frequency: u64,
    #[prost(uint64, tag = "18")]
    cancellation_generation: u64,
    #[prost(string, tag = "19")]
    capture_session_id: String,
    #[prost(uint32, tag = "20")]
    selected_process_id: u32,
    #[prost(fixed64, tag = "21")]
    selected_window_handle: u64,
    #[prost(string, tag = "22")]
    selected_executable_name: String,
    #[prost(uint64, tag = "23")]
    source_device_generation: u64,
    #[prost(uint64, tag = "24")]
    source_geometry_epoch: u64,
    #[prost(string, tag = "25")]
    picker_consent_token: String,
    #[prost(string, tag = "26")]
    game_profile_id: String,
    #[prost(string, tag = "27")]
    character_id: String,
    #[prost(string, tag = "28")]
    subject_id: String,
    #[prost(string, tag = "29")]
    reference_id: String,
    #[prost(string, tag = "30")]
    subject_display_name: String,
    #[prost(enumeration = "IdentityReferenceSourceClassPayload", tag = "31")]
    source_class: i32,
    #[prost(string, tag = "32")]
    owner_user_id: String,
    #[prost(string, tag = "33")]
    original_work_license: String,
    #[prost(bool, tag = "34")]
    explicit_user_consent: bool,
    #[prost(bool, tag = "35")]
    local_only: bool,
    #[prost(uint64, tag = "36")]
    imported_at_unix_ms: u64,
}

#[derive(Clone, PartialEq, Message)]
struct IdentityFrameLeasePayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(uint32, tag = "2")]
    worker_process_id: u32,
    #[prost(fixed64, tag = "3")]
    worker_process_creation_time: u64,
    #[prost(string, tag = "4")]
    worker_executable_name: String,
    #[prost(string, tag = "5")]
    lease_id: String,
    #[prost(string, tag = "6")]
    shared_memory_name: String,
    #[prost(string, tag = "7")]
    lease_nonce: String,
    #[prost(uint64, tag = "8")]
    byte_length: u64,
    #[prost(uint32, tag = "9")]
    width: u32,
    #[prost(uint32, tag = "10")]
    height: u32,
    #[prost(uint32, tag = "11")]
    stride_bytes: u32,
    #[prost(string, tag = "12")]
    pixel_format: String,
    #[prost(string, tag = "13")]
    content_sha256: String,
    #[prost(uint64, tag = "14")]
    expires_qpc: u64,
    #[prost(uint64, tag = "15")]
    qpc_frequency: u64,
    #[prost(uint64, tag = "16")]
    cancellation_generation: u64,
    #[prost(string, tag = "17")]
    capture_session_id: String,
    #[prost(uint32, tag = "18")]
    selected_process_id: u32,
    #[prost(fixed64, tag = "19")]
    selected_window_handle: u64,
    #[prost(string, tag = "20")]
    selected_executable_name: String,
    #[prost(uint64, tag = "21")]
    source_device_generation: u64,
    #[prost(uint64, tag = "22")]
    source_geometry_epoch: u64,
    #[prost(uint64, tag = "23")]
    source_frame_sequence: u64,
    #[prost(uint64, tag = "24")]
    source_frame_qpc: u64,
    #[prost(uint64, tag = "25")]
    captured_at_unix_ms: u64,
    #[prost(bool, tag = "26")]
    advancing_frame_verified: bool,
    #[prost(bool, tag = "27")]
    overlay_capture_excluded: bool,
    #[prost(bool, tag = "28")]
    protected_online_detected: bool,
    #[prost(bool, tag = "29")]
    anti_cheat_detected: bool,
    #[prost(uint32, tag = "30")]
    crop_left: u32,
    #[prost(uint32, tag = "31")]
    crop_top: u32,
    #[prost(uint32, tag = "32")]
    crop_right: u32,
    #[prost(uint32, tag = "33")]
    crop_bottom: u32,
    #[prost(uint32, tag = "34")]
    source_width: u32,
    #[prost(uint32, tag = "35")]
    source_height: u32,
}

#[derive(Clone, PartialEq, Message)]
struct ReleaseIdentityFramePayload {
    #[prost(uint32, tag = "1")]
    worker_process_id: u32,
    #[prost(string, tag = "2")]
    lease_id: String,
    #[prost(string, tag = "3")]
    lease_nonce: String,
}

#[derive(Clone, PartialEq, Message)]
struct AllocateVisualSourcePayload {
    #[prost(uint32, tag = "1")]
    worker_process_id: u32,
    #[prost(fixed64, tag = "2")]
    worker_process_creation_time: u64,
    #[prost(string, tag = "3")]
    worker_executable_name: String,
    #[prost(uint64, tag = "4")]
    actor_id: u64,
    #[prost(uint64, tag = "5")]
    track_id: u64,
    #[prost(uint64, tag = "6")]
    track_epoch: u64,
}

#[derive(Clone, PartialEq, Message)]
struct VisualSourceLeasePayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(uint32, tag = "2")]
    broker_process_id: u32,
    #[prost(fixed64, tag = "3")]
    broker_process_creation_time: u64,
    #[prost(string, tag = "4")]
    broker_executable_name: String,
    #[prost(uint32, tag = "5")]
    worker_process_id: u32,
    #[prost(fixed64, tag = "6")]
    worker_process_creation_time: u64,
    #[prost(string, tag = "7")]
    worker_executable_name: String,
    #[prost(fixed64, tag = "8")]
    worker_handle_value: u64,
    #[prost(fixed64, tag = "9")]
    lease_nonce_high: u64,
    #[prost(fixed64, tag = "10")]
    lease_nonce_low: u64,
    #[prost(fixed64, tag = "11")]
    adapter_luid: u64,
    #[prost(fixed64, tag = "12")]
    keyed_mutex_acquire_key: u64,
    #[prost(fixed64, tag = "13")]
    keyed_mutex_release_key: u64,
    #[prost(uint32, tag = "14")]
    width: u32,
    #[prost(uint32, tag = "15")]
    height: u32,
    #[prost(uint32, tag = "16")]
    stride_bytes: u32,
    #[prost(uint32, tag = "17")]
    dxgi_format: u32,
    #[prost(uint32, tag = "18")]
    alpha_mode: u32,
    #[prost(uint64, tag = "19")]
    expires_qpc: u64,
    #[prost(uint64, tag = "20")]
    qpc_frequency: u64,
    #[prost(uint64, tag = "21")]
    cancellation_generation: u64,
    #[prost(uint64, tag = "22")]
    source_device_generation: u64,
    #[prost(uint64, tag = "23")]
    source_geometry_epoch: u64,
    #[prost(uint64, tag = "24")]
    source_frame_sequence: u64,
    #[prost(uint64, tag = "25")]
    source_frame_qpc: u64,
    #[prost(uint64, tag = "26")]
    actor_id: u64,
    #[prost(uint64, tag = "27")]
    track_id: u64,
    #[prost(uint64, tag = "28")]
    track_epoch: u64,
}

#[derive(Clone, PartialEq, Message)]
struct ReleaseVisualSourcePayload {
    #[prost(uint32, tag = "1")]
    worker_process_id: u32,
    #[prost(fixed64, tag = "2")]
    lease_nonce_high: u64,
    #[prost(fixed64, tag = "3")]
    lease_nonce_low: u64,
}

#[derive(Clone, PartialEq, Message)]
struct SubmitOcclusionPayload {
    #[prost(double, tag = "1")]
    face_confidence: f64,
    #[prost(double, tag = "2")]
    landmark_confidence: f64,
    #[prost(double, tag = "3")]
    visibility_ratio: f64,
    #[prost(bool, tag = "4")]
    mouth_occluded: bool,
    #[prost(uint64, tag = "5")]
    measured_qpc: u64,
    #[prost(uint64, tag = "6")]
    source_frame_sequence: u64,
    #[prost(uint64, tag = "7")]
    source_device_generation: u64,
    #[prost(uint64, tag = "9")]
    track_epoch: u64,
    #[prost(uint64, tag = "10")]
    source_frame_qpc: u64,
    #[prost(uint64, tag = "11")]
    actor_id: u64,
    #[prost(uint64, tag = "12")]
    track_id: u64,
    #[prost(uint64, tag = "13")]
    source_geometry_epoch: u64,
}

#[derive(Clone, PartialEq, Message)]
struct SharedTexturePayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(bytes = "vec", tag = "2")]
    session_nonce: Vec<u8>,
    #[prost(string, tag = "3")]
    session_id: String,
    #[prost(fixed64, tag = "4")]
    lease_nonce_high: u64,
    #[prost(fixed64, tag = "5")]
    lease_nonce_low: u64,
    #[prost(uint32, tag = "6")]
    worker_process_id: u32,
    #[prost(fixed64, tag = "7")]
    worker_process_creation_time: u64,
    #[prost(string, tag = "8")]
    worker_executable_name: String,
    #[prost(fixed64, tag = "9")]
    source_process_handle_value: u64,
    #[prost(fixed64, tag = "10")]
    adapter_luid: u64,
    #[prost(fixed64, tag = "11")]
    keyed_mutex_acquire_key: u64,
    #[prost(fixed64, tag = "12")]
    keyed_mutex_release_key: u64,
    #[prost(uint32, tag = "13")]
    width: u32,
    #[prost(uint32, tag = "14")]
    height: u32,
    #[prost(uint32, tag = "15")]
    stride_bytes: u32,
    #[prost(uint32, tag = "16")]
    dxgi_format: u32,
    #[prost(uint32, tag = "17")]
    alpha_mode: u32,
    #[prost(uint64, tag = "18")]
    expires_qpc: u64,
}

#[derive(Clone, PartialEq, Message)]
struct SubmitPatchPayload {
    #[prost(uint64, tag = "1")]
    source_frame_sequence: u64,
    #[prost(uint64, tag = "2")]
    cancellation_generation: u64,
    #[prost(double, tag = "3")]
    left: f64,
    #[prost(double, tag = "4")]
    top: f64,
    #[prost(double, tag = "5")]
    right: f64,
    #[prost(double, tag = "6")]
    bottom: f64,
    #[prost(double, tag = "7")]
    confidence: f64,
    #[prost(uint64, tag = "8")]
    produced_qpc: u64,
    #[prost(message, optional, tag = "9")]
    shared_texture: Option<SharedTexturePayload>,
    #[prost(uint64, tag = "10")]
    source_device_generation: u64,
    #[prost(uint64, tag = "11")]
    source_frame_qpc: u64,
    #[prost(uint64, tag = "13")]
    track_epoch: u64,
    #[prost(uint64, tag = "14")]
    actor_id: u64,
    #[prost(uint64, tag = "15")]
    track_id: u64,
    #[prost(uint64, tag = "16")]
    source_geometry_epoch: u64,
}

#[derive(Clone, PartialEq, Message)]
struct AllocatePlaybackStreamPayload {
    #[prost(string, tag = "1")]
    session_id: String,
    #[prost(string, tag = "2")]
    turn_id: String,
    #[prost(uint64, tag = "3")]
    generation: u64,
    #[prost(uint32, tag = "4")]
    sample_rate: u32,
    #[prost(uint32, tag = "5")]
    channels: u32,
    #[prost(uint64, tag = "6")]
    max_frames: u64,
    #[prost(uint32, tag = "7")]
    expected_producer_process_id: u32,
}

#[derive(Clone, PartialEq, Message)]
struct PlaybackLeasePayload {
    #[prost(uint32, tag = "1")]
    schema_version: u32,
    #[prost(string, tag = "2")]
    stream_id: String,
    #[prost(string, tag = "3")]
    producer_endpoint: String,
    #[prost(bytes = "vec", tag = "4")]
    one_time_token: Vec<u8>,
    #[prost(string, tag = "5")]
    session_id: String,
    #[prost(string, tag = "6")]
    turn_id: String,
    #[prost(uint64, tag = "7")]
    generation: u64,
    #[prost(uint32, tag = "8")]
    sample_rate: u32,
    #[prost(uint32, tag = "9")]
    channels: u32,
    #[prost(uint64, tag = "10")]
    max_frames: u64,
    #[prost(uint32, tag = "11")]
    max_chunk_bytes: u32,
    #[prost(uint64, tag = "12")]
    expires_qpc: u64,
    #[prost(enumeration = "AudioOutputSelectionModePayload", tag = "13")]
    output_selection_mode: i32,
    #[prost(string, tag = "14")]
    output_endpoint_id: String,
    #[prost(uint64, tag = "15")]
    output_endpoint_generation: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AudioOutputSelectionMode {
    SystemDefault,
    EndpointId,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum AudioOutputSelection {
    SystemDefault,
    EndpointId { endpoint_id: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AudioOutputState {
    Active,
    Disabled,
    NotPresent,
    Unplugged,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioOutputEndpoint {
    pub endpoint_id: String,
    pub friendly_name: String,
    pub state: AudioOutputState,
    pub system_default: bool,
    pub generation: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioOutputSnapshot {
    pub schema_version: u32,
    pub catalog_generation: u64,
    pub endpoints: Vec<AudioOutputEndpoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedAudioOutput {
    pub schema_version: u32,
    pub selection: AudioOutputSelection,
    pub resolved: AudioOutputEndpoint,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum AudioInputSelection {
    SystemDefault,
    EndpointId { endpoint_id: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AudioInputState {
    Active,
    Disabled,
    NotPresent,
    Unplugged,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputEndpoint {
    pub endpoint_id: String,
    pub friendly_name: String,
    pub state: AudioInputState,
    pub system_default: bool,
    pub generation: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputSnapshot {
    pub schema_version: u32,
    pub catalog_generation: u64,
    pub endpoints: Vec<AudioInputEndpoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedAudioInput {
    pub schema_version: u32,
    pub selection: AudioInputSelection,
    pub resolved: AudioInputEndpoint,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrustedCaptureBackend {
    WindowsGraphicsCapture,
    DesktopDuplication,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrustedCaptureScope {
    ExactGameHwndWgc,
    MonitorRegionCrop,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrustedTargetColorSpace {
    SdrSrgb,
    SdrScRgb,
    Hdr10Pq,
    HdrScRgb,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PresentationPixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrustedSubtitlePresentationContext {
    pub schema_version: u32,
    pub selected_process_id: u32,
    pub selected_window: u64,
    pub selected_executable_name: String,
    pub capture_device_generation: u64,
    pub geometry_epoch: u64,
    pub source_frame_sequence: u64,
    pub source_frame_qpc: u64,
    pub window_bounds_px: PresentationPixelRect,
    pub client_bounds_px: PresentationPixelRect,
    pub captured_width_px: u32,
    pub captured_height_px: u32,
    pub monitor_id: String,
    pub monitor_bounds_px: PresentationPixelRect,
    pub monitor_work_area_px: PresentationPixelRect,
    pub dpi_available: bool,
    pub dpi_x: u32,
    pub dpi_y: u32,
    pub hdr_evidence_available: bool,
    pub hdr_supported: bool,
    pub hdr_user_enabled: bool,
    pub hdr_active: bool,
    pub advanced_color_active: bool,
    pub active_color_mode: u32,
    pub color_encoding_available: bool,
    pub color_encoding: u32,
    pub bits_per_color_channel: u32,
    pub sdr_white_level_available: bool,
    pub sdr_white_level_nits: f64,
    pub capture_backend: TrustedCaptureBackend,
    pub capture_scope: TrustedCaptureScope,
    pub overlay_capture_excluded: bool,
    pub overlay_visuals_allowed: bool,
    pub target_color_space_available: bool,
    pub target_color_space: Option<TrustedTargetColorSpace>,
    pub attested_at_qpc: u64,
    pub qpc_frequency: u64,
    pub attestation_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VisualAudioEnvelopeQuery {
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub stream_id: String,
    /// A playback lease is one sentence segment, so the canonical segment ID
    /// is its one-use stream ID.
    pub segment_id: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VisualSpeechCue {
    pub start_sample: u64,
    pub duration_samples: u64,
    pub canonical_viseme: u8,
    pub strength_q15: u16,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VisualAudioEnvelope {
    pub schema_version: u32,
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub stream_id: String,
    pub segment_id: String,
    pub source_sample_start: u64,
    pub source_sample_count: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub device_write_qpc: u64,
    pub qpc_frequency: u64,
    pub source_frames: u64,
    pub device_frames: u64,
    pub mono_rms_q15: [u16; 8],
    pub mono_peak_q15: [u16; 8],
    pub active: bool,
    pub draining: bool,
    pub cancelled: bool,
    pub visual_speech_cues: Vec<VisualSpeechCue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AudioInputRehearsalRequest {
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub duration_ms: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub max_frames: u64,
    pub activation_source: InputActivationSource,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum InputActivationSource {
    ExplicitRehearsal,
    PushToTalk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PttActivationStateKind {
    Released,
    Pressed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PttActivationState {
    pub schema_version: u32,
    pub virtual_key: u32,
    pub state: PttActivationStateKind,
    pub transition_sequence: u64,
    pub transition_qpc: u64,
    pub release_transition_sequence: u64,
    pub released_qpc: u64,
}

impl PttActivationState {
    pub(crate) fn is_strictly_newer_press_than(self, baseline: Self) -> bool {
        self.schema_version == baseline.schema_version
            && self.virtual_key == baseline.virtual_key
            && self.state == PttActivationStateKind::Pressed
            && self.transition_sequence > baseline.transition_sequence
            && self.transition_qpc > baseline.transition_qpc
    }
}

#[derive(PartialEq, Eq)]
pub(crate) struct InputLeaseToken([u8; 32]);

impl Drop for InputLeaseToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for InputLeaseToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InputLeaseToken([REDACTED])")
    }
}

impl Serialize for InputLeaseToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut encoded = Zeroizing::new(String::with_capacity(64));
        for byte in self.0 {
            use std::fmt::Write as _;
            write!(&mut encoded, "{byte:02x}").map_err(serde::ser::Error::custom)?;
        }
        serializer.serialize_str(&encoded)
    }
}

#[derive(Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioInputRehearsalLease {
    pub schema_version: u32,
    pub stream_id: String,
    pub producer_endpoint: String,
    pub one_time_token: InputLeaseToken,
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub duration_ms: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub max_frames: u64,
    pub max_chunk_bytes: u32,
    pub expires_qpc: u64,
    pub qpc_frequency: u64,
    pub input_selection_mode: AudioOutputSelectionMode,
    pub input_endpoint_id: String,
    pub input_endpoint_generation: u64,
    pub activation_source: InputActivationSource,
    pub ptt_virtual_key: u32,
    pub ptt_press_transition_sequence: u64,
    pub ptt_pressed_qpc: u64,
}

impl std::fmt::Debug for AudioInputRehearsalLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioInputRehearsalLease")
            .field("schema_version", &self.schema_version)
            .field("stream_id", &self.stream_id)
            .field("producer_endpoint", &self.producer_endpoint)
            .field("one_time_token", &"[REDACTED]")
            .field("session_id", &self.session_id)
            .field("turn_id", &self.turn_id)
            .field("generation", &self.generation)
            .field("duration_ms", &self.duration_ms)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("max_frames", &self.max_frames)
            .field("max_chunk_bytes", &self.max_chunk_bytes)
            .field("expires_qpc", &self.expires_qpc)
            .field("qpc_frequency", &self.qpc_frequency)
            .field("input_selection_mode", &self.input_selection_mode)
            .field("input_endpoint_id", &self.input_endpoint_id)
            .field("input_endpoint_generation", &self.input_endpoint_generation)
            .field("activation_source", &self.activation_source)
            .field("ptt_virtual_key", &self.ptt_virtual_key)
            .field(
                "ptt_press_transition_sequence",
                &self.ptt_press_transition_sequence,
            )
            .field("ptt_pressed_qpc", &self.ptt_pressed_qpc)
            .finish()
    }
}

#[derive(PartialEq, Eq)]
pub struct PlaybackToken([u8; 32]);

impl Drop for PlaybackToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for PlaybackToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PlaybackToken([REDACTED])")
    }
}

impl Serialize for PlaybackToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut encoded = Zeroizing::new(String::with_capacity(64));
        for byte in self.0 {
            use std::fmt::Write as _;
            write!(&mut encoded, "{byte:02x}").map_err(serde::ser::Error::custom)?;
        }
        serializer.serialize_str(&encoded)
    }
}

#[derive(Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioPlaybackLease {
    pub schema_version: u32,
    pub stream_id: String,
    pub producer_endpoint: String,
    pub one_time_token: PlaybackToken,
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub max_frames: u64,
    pub max_chunk_bytes: u32,
    pub expires_qpc: u64,
    pub output_selection_mode: AudioOutputSelectionMode,
    pub output_endpoint_id: String,
    pub output_endpoint_generation: u64,
}

#[cfg(test)]
pub(crate) fn test_audio_playback_lease(
    stream_id: impl Into<String>,
    generation: u64,
) -> AudioPlaybackLease {
    AudioPlaybackLease {
        schema_version: 2,
        stream_id: stream_id.into(),
        producer_endpoint: "fixture-producer".into(),
        one_time_token: PlaybackToken([0x2a; 32]),
        session_id: "fixture-session".into(),
        turn_id: "fixture-turn".into(),
        generation,
        sample_rate: 24_000,
        channels: 1,
        max_frames: 24_000,
        max_chunk_bytes: 64 * 1024,
        expires_qpc: u64::MAX,
        output_selection_mode: AudioOutputSelectionMode::SystemDefault,
        output_endpoint_id: String::new(),
        output_endpoint_generation: 1,
    }
}

impl std::fmt::Debug for AudioPlaybackLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioPlaybackLease")
            .field("schema_version", &self.schema_version)
            .field("stream_id", &self.stream_id)
            .field("producer_endpoint", &self.producer_endpoint)
            .field("one_time_token", &"[REDACTED]")
            .field("session_id", &self.session_id)
            .field("turn_id", &self.turn_id)
            .field("generation", &self.generation)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("max_frames", &self.max_frames)
            .field("max_chunk_bytes", &self.max_chunk_bytes)
            .field("expires_qpc", &self.expires_qpc)
            .field("output_selection_mode", &self.output_selection_mode)
            .field("output_endpoint_id", &self.output_endpoint_id)
            .field(
                "output_endpoint_generation",
                &self.output_endpoint_generation,
            )
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioPlaybackPoolRequest {
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub max_frames_per_lease: u64,
    pub lease_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualWorkerIdentity {
    pub process_id: u32,
    pub process_creation_time: u64,
    pub executable_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualTrackBinding {
    pub actor_id: u64,
    pub track_id: u64,
    pub track_epoch: u64,
}

/// One detected actor bound to an exact current native visual-authority frame.
///
/// This type intentionally has no serde implementation. Bounds, track IDs,
/// and candidate membership are native-only and must never cross Tauri IPC.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeManualActorCandidateV1 {
    pub actor_id: u64,
    pub track_id: u64,
    pub track_epoch: u64,
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// Private begin request produced only by an admitted visual detector.
/// Captured pixels, HWNDs, candidates, and coordinates stay inside Rust/native
/// code; the WebView can express only the singleton start/status/cancel intent.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeManualActorPickerRequestV1 {
    pub request_id: String,
    pub visual_pack_id: String,
    pub visual_pack_admission_sha256: String,
    pub game_profile_id: String,
    pub capture_session_id: String,
    pub cancellation_generation: u64,
    pub selected_process_id: u32,
    pub selected_window_handle: u64,
    pub selected_executable_name: String,
    pub source_device_generation: u64,
    pub source_geometry_epoch: u64,
    pub source_frame_sequence: u64,
    pub source_frame_qpc: u64,
    pub qpc_frequency: u64,
    pub captured_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub source_width: u32,
    pub source_height: u32,
    pub timeout_ms: u32,
    pub candidates: Vec<NativeManualActorCandidateV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeManualActorPickerStatusV1 {
    Pending,
    Selected,
    Cancelled,
    TimedOut,
    TargetLost,
    TargetResized,
    DpiChanged,
    DeviceChanged,
    CaptureChanged,
    ClickOutsideDetectedRoi,
    AmbiguousDetectedRoi,
    UntrustedPointerInput,
    OverlayUnavailable,
    InternalError,
}

/// Opaque terminal/pending receipt from command 31. It is neither serializable
/// nor debug-printable with its private provenance. Only the actor-lock bridge
/// is allowed to consume the selected form.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct NativeManualActorPickerReceiptV1 {
    pub schema_version: u32,
    pub request_id: String,
    pub status: NativeManualActorPickerStatusV1,
    pub receipt_nonce_high: u64,
    pub receipt_nonce_low: u64,
    pub capture_session_id: String,
    pub cancellation_generation: u64,
    pub selected_process_id: u32,
    pub selected_window_handle: u64,
    pub selected_executable_name: String,
    pub source_device_generation: u64,
    pub source_geometry_epoch: u64,
    pub source_frame_sequence: u64,
    pub source_frame_qpc: u64,
    pub selected_actor_id: u64,
    pub selected_track_id: u64,
    pub selected_track_epoch: u64,
    pub candidate_count: u32,
    pub candidate_set_sha256: String,
    pub began_qpc: u64,
    pub clicked_qpc: u64,
    pub attested_at_qpc: u64,
    pub qpc_frequency: u64,
    pub pointer_kind: u32,
    pub frozen_wgc_frame_verified: bool,
    pub overlay_capture_excluded: bool,
    pub overlay_nonactivating: bool,
    pub single_hardware_pointer_click: bool,
    pub pixels_withheld_from_webview: bool,
    pub coordinates_withheld_from_webview: bool,
    pub receipt_sha256: String,
}

impl std::fmt::Debug for NativeManualActorPickerReceiptV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeManualActorPickerReceiptV1")
            .field("schema_version", &self.schema_version)
            .field("request_id", &"[REDACTED]")
            .field("status", &self.status)
            .field("native_provenance", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualSourceLease {
    pub schema_version: u32,
    pub broker_process_id: u32,
    pub broker_process_creation_time: u64,
    pub broker_executable_name: String,
    pub worker: VisualWorkerIdentity,
    pub worker_handle_value: u64,
    pub lease_nonce_high: u64,
    pub lease_nonce_low: u64,
    pub adapter_luid: u64,
    pub keyed_mutex_acquire_key: u64,
    pub keyed_mutex_release_key: u64,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub dxgi_format: u32,
    pub alpha_mode: u32,
    pub expires_qpc: u64,
    pub qpc_frequency: u64,
    pub cancellation_generation: u64,
    pub source_device_generation: u64,
    pub source_geometry_epoch: u64,
    pub source_frame_sequence: u64,
    pub source_frame_qpc: u64,
    pub track: VisualTrackBinding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityWorkerIdentity {
    pub process_id: u32,
    pub process_creation_time: u64,
    pub executable_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentityCrop {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl IdentityCrop {
    pub fn width(self) -> u32 {
        self.right.saturating_sub(self.left)
    }

    pub fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }
}

/// One exact WGC crop held in a broker-owned, same-user read-only mapping.
///
/// This type is intentionally not serializable. The mapping name and nonce are
/// process-local capabilities used only to construct the hidden worker request;
/// they must never cross the WebView boundary or be written to diagnostics.
pub struct IdentityFrameLease {
    pub schema_version: u32,
    pub worker: IdentityWorkerIdentity,
    pub(crate) lease_id: Zeroizing<String>,
    pub(crate) shared_memory_name: Zeroizing<String>,
    pub(crate) lease_nonce: Zeroizing<String>,
    pub byte_length: u64,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub pixel_format: String,
    pub content_sha256: String,
    pub expires_qpc: u64,
    pub qpc_frequency: u64,
    pub cancellation_generation: u64,
    pub capture_session_id: String,
    pub selected_process_id: u32,
    pub selected_window_handle: u64,
    pub selected_executable_name: String,
    pub source_device_generation: u64,
    pub source_geometry_epoch: u64,
    pub source_frame_sequence: u64,
    pub source_frame_qpc: u64,
    pub captured_at_unix_ms: u64,
    pub advancing_frame_verified: bool,
    pub overlay_capture_excluded: bool,
    pub protected_online_detected: bool,
    pub anti_cheat_detected: bool,
    pub crop: IdentityCrop,
    pub source_width: u32,
    pub source_height: u32,
}

impl IdentityFrameLease {
    pub(crate) fn lease_id(&self) -> &str {
        self.lease_id.as_str()
    }

    pub(crate) fn shared_memory_name(&self) -> &str {
        self.shared_memory_name.as_str()
    }

    pub(crate) fn lease_nonce(&self) -> &str {
        self.lease_nonce.as_str()
    }
}

impl std::fmt::Debug for IdentityFrameLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IdentityFrameLease")
            .field("schema_version", &self.schema_version)
            .field("worker", &self.worker)
            .field("lease_id", &"[REDACTED]")
            .field("shared_memory_name", &"[REDACTED]")
            .field("lease_nonce", &"[REDACTED]")
            .field("byte_length", &self.byte_length)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("content_sha256", &self.content_sha256)
            .field("capture_session_id", &self.capture_session_id)
            .field("selected_process_id", &self.selected_process_id)
            .field("selected_window_handle", &self.selected_window_handle)
            .field("source_frame_sequence", &self.source_frame_sequence)
            .field("source_frame_qpc", &self.source_frame_qpc)
            .field("crop", &self.crop)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IdentityReferenceSourceClass {
    UserPrivate,
    OriginalSynthetic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IdentityReferenceImportRequest {
    pub worker: IdentityWorkerIdentity,
    pub picker_consent_token: String,
    pub game_profile_id: String,
    pub character_id: String,
    pub subject_id: String,
    pub reference_id: String,
    pub subject_display_name: String,
    pub source_class: IdentityReferenceSourceClass,
    pub owner_user_id: Option<String>,
    pub original_work_license: Option<String>,
    pub explicit_user_consent: bool,
    pub local_only: bool,
    pub imported_at_unix_ms: u64,
}

pub(crate) struct IdentityReferenceImportLease {
    pub schema_version: u32,
    pub worker: IdentityWorkerIdentity,
    pub(crate) lease_id: Zeroizing<String>,
    pub(crate) shared_memory_name: Zeroizing<String>,
    pub(crate) lease_nonce: Zeroizing<String>,
    pub byte_length: u64,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub pixel_format: String,
    pub content_sha256: String,
    pub source_asset_sha256: String,
    pub source_media_type: String,
    pub expires_qpc: u64,
    pub qpc_frequency: u64,
    pub cancellation_generation: u64,
    pub capture_session_id: String,
    pub selected_process_id: u32,
    pub selected_window_handle: u64,
    pub selected_executable_name: String,
    pub source_device_generation: u64,
    pub source_geometry_epoch: u64,
    pub picker_consent_token: String,
    pub game_profile_id: String,
    pub character_id: String,
    pub subject_id: String,
    pub reference_id: String,
    pub subject_display_name: String,
    pub source_class: IdentityReferenceSourceClass,
    pub owner_user_id: Option<String>,
    pub original_work_license: Option<String>,
    pub explicit_user_consent: bool,
    pub local_only: bool,
    pub imported_at_unix_ms: u64,
}

impl IdentityReferenceImportLease {
    pub(crate) fn lease_id(&self) -> &str {
        self.lease_id.as_str()
    }
    pub(crate) fn shared_memory_name(&self) -> &str {
        self.shared_memory_name.as_str()
    }
    pub(crate) fn lease_nonce(&self) -> &str {
        self.lease_nonce.as_str()
    }
}

impl std::fmt::Debug for IdentityReferenceImportLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IdentityReferenceImportLease")
            .field("schema_version", &self.schema_version)
            .field("worker", &self.worker)
            .field("lease_id", &"[REDACTED]")
            .field("shared_memory_name", &"[REDACTED]")
            .field("lease_nonce", &"[REDACTED]")
            .field("byte_length", &self.byte_length)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("source_media_type", &self.source_media_type)
            .field("capture_session_id", &self.capture_session_id)
            .field("selected_process_id", &self.selected_process_id)
            .field("selected_window_handle", &self.selected_window_handle)
            .field("game_profile_id", &self.game_profile_id)
            .field("character_id", &self.character_id)
            .field("subject_id", &self.subject_id)
            .field("reference_id", &self.reference_id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrokerOcclusionEvidence {
    pub face_confidence: f64,
    pub landmark_confidence: f64,
    pub visibility_ratio: f64,
    pub mouth_occluded: bool,
    pub measured_qpc: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrokerResidualProposal {
    pub worker: VisualWorkerIdentity,
    pub lease_nonce_high: u64,
    pub lease_nonce_low: u64,
    pub worker_handle_value: u64,
    pub adapter_luid: u64,
    pub keyed_mutex_acquire_key: u64,
    pub keyed_mutex_release_key: u64,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub dxgi_format: u32,
    pub alpha_mode: u32,
    pub expires_qpc: u64,
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub confidence: f64,
    pub produced_qpc: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrokerPresentationReceipt {
    pub schema_version: u32,
    pub response_sequence: u64,
    pub cancellation_generation: u64,
    pub source_frame_sequence: u64,
    pub source_frame_qpc: u64,
    pub actor_id: u64,
    pub track_id: u64,
    pub track_epoch: u64,
    pub residual_contract_status: u32,
    pub presented: bool,
    pub failure_code: Option<u32>,
}

impl BrokerCommand {
    fn advances_cancellation_generation_on_success(self) -> bool {
        if matches!(self, Self::SelectTarget | Self::ClearTarget) {
            return true;
        }
        false
    }
}

#[derive(Clone, PartialEq, Message)]
struct SelectTargetPayload {
    #[prost(uint64, tag = "1")]
    native_window: u64,
    #[prost(uint32, tag = "2")]
    expected_process_id: u32,
    #[prost(string, repeated, tag = "3")]
    allowed_process_names: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CapturePixelSource {
    WindowsGraphicsCaptureTexture,
    DesktopDuplicationTexture,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CapturePixelScope {
    ExactSelectedWindow,
    FullDisplayOutput,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeCaptureEvidence {
    pub schema_version: u32,
    pub selected_process_id: u32,
    pub selected_window_handle: u64,
    pub device_generation: u64,
    pub geometry_epoch: u64,
    pub latest_frame_sequence: u64,
    pub latest_frame_qpc: u64,
    pub initial_content_hash: u64,
    pub latest_content_hash: u64,
    pub content_hash_changes: u64,
    pub geometry_changes: u64,
    pub nonadvancing_frames: u64,
    pub content_width: u32,
    pub content_height: u32,
    pub overlay_capture_excluded: bool,
    pub overlay_visuals_allowed: bool,
    pub pixel_source: CapturePixelSource,
    pub pixel_scope: CapturePixelScope,
    /// Exact-window WGC pixels exclude unrelated per-display layered overlays.
    /// This is a source/scope property, not process enumeration or a claim
    /// about whether a particular dimmer is currently running.
    pub external_display_overlay_pixels_excluded: bool,
    pub desktop_luminance_excluded_from_pixel_evidence: bool,
    /// External display overlays may change perceived whole-display brightness,
    /// independently of the exact selected-HWND WGC texture used as pixel truth.
    pub external_display_overlays_may_change_perceived_brightness: bool,
    pub selected_executable_name: String,
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct DebugSyntheticTargetIdentity {
    native_window: u64,
    process_id: u32,
    executable_basename: String,
    portrait_sha256: String,
    qualification_mode: DebugSyntheticQualificationMode,
}

#[cfg(debug_assertions)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DebugSyntheticQualificationMode {
    StaticCompatibility,
    MovingSourceControlledIdle,
}

#[cfg(debug_assertions)]
impl DebugSyntheticQualificationMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::StaticCompatibility => "static_compatibility_v1",
            Self::MovingSourceControlledIdle => "moving_source_controlled_idle_v2",
        }
    }
}

#[cfg(debug_assertions)]
#[derive(Debug, Deserialize)]
struct DebugSyntheticTargetMetadata {
    schema_version: u32,
    fixture_kind: String,
    state: String,
    pid: u32,
    process_id: u32,
    window_handle: i64,
    hwnd: i64,
    executable_basename: String,
    exe_basename: String,
    decoded_frames: u64,
    visual_source: String,
    portrait_sha256: String,
    #[serde(default)]
    source_mouth_motion: Option<bool>,
    #[serde(default)]
    source_sequence_sha256: Option<String>,
    #[serde(default)]
    source_frame_count: Option<u32>,
    #[serde(default)]
    source_frame_width: Option<u32>,
    #[serde(default)]
    source_frame_height: Option<u32>,
    #[serde(default)]
    source_frame_rate: Option<u32>,
    #[serde(default)]
    source_frame_motion: Option<bool>,
    #[serde(default)]
    source_actor_motion: Option<bool>,
    #[serde(default)]
    rendered_actor_motion: Option<bool>,
    #[serde(default)]
    rendered_blink_motion: Option<bool>,
    #[serde(default)]
    rendered_breathing_motion: Option<bool>,
    #[serde(default)]
    source_mouth_articulation: Option<bool>,
    #[serde(default)]
    product_lip_sync: Option<bool>,
    #[serde(default)]
    visual_mode: Option<String>,
    #[serde(default)]
    content_frame_index: Option<u32>,
    #[serde(default)]
    content_frame_sha256: Option<String>,
}

#[cfg(debug_assertions)]
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DebugSyntheticReplayCaptureSnapshot {
    pub target_process_id: u32,
    pub target_window_handle: u64,
    pub target_executable_basename: String,
    pub fixture_motion_mode: String,
    pub diagnostics: MediaBrokerDiagnostics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_evidence: Option<NativeCaptureEvidence>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum BrokerStatus {
    Ok = 0,
    InvalidFrame = 1,
    UnsupportedVersion = 2,
    AuthenticationFailed = 3,
    SessionMismatch = 4,
    SequenceReplayed = 5,
    DeadlineExpired = 6,
    DeadlineTooFar = 7,
    CancellationMismatch = 8,
    PayloadInvalid = 9,
    TargetBlocked = 10,
    CapabilityUnavailable = 11,
    InternalError = 12,
}

struct BrokerConnection {
    stream: BrokerStream,
    nonce: [u8; 32],
    session_id: String,
    next_sequence: u64,
    cancellation_generation: u64,
}

type BrokerStream = std::fs::File;

#[derive(Clone)]
struct BrokerClient(Arc<Mutex<BrokerConnection>>);

impl std::fmt::Debug for BrokerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BrokerClient { authenticated: true }")
    }
}

impl BrokerClient {
    fn new(stream: BrokerStream, nonce: [u8; 32], session_id: String) -> Self {
        Self(Arc::new(Mutex::new(BrokerConnection {
            stream,
            nonce,
            session_id,
            next_sequence: 1,
            cancellation_generation: 0,
        })))
    }

    async fn health(&self) -> Result<BrokerHealth, MediaBrokerError> {
        let response = self.request(BrokerCommand::Health).await?;
        if response.payload.len() != 12 {
            return Err(MediaBrokerError::Malformed);
        }
        Ok(BrokerHealth {
            broker_state: read_u32(&response.payload, 8)?,
        })
    }

    async fn diagnostics(&self) -> Result<MediaBrokerDiagnostics, MediaBrokerError> {
        let response = self.request(BrokerCommand::Diagnostics).await?;
        decode_diagnostics(&response.payload)
    }

    async fn enumerate_audio_outputs(&self) -> Result<AudioOutputSnapshot, MediaBrokerError> {
        let response = self.request(BrokerCommand::EnumerateAudioOutputs).await?;
        let payload = AudioOutputSnapshotPayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_audio_output_snapshot(payload)
    }

    async fn select_audio_output(
        &self,
        selection: &AudioOutputSelection,
    ) -> Result<SelectedAudioOutput, MediaBrokerError> {
        let payload = encode_audio_output_selection(selection)?;
        let response = self
            .request_with_payload(BrokerCommand::SelectAudioOutput, payload.encode_to_vec())
            .await?;
        let selected = SelectedAudioOutputPayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_selected_audio_output(selected)
    }

    async fn selected_audio_output(&self) -> Result<SelectedAudioOutput, MediaBrokerError> {
        let response = self.request(BrokerCommand::SelectedAudioOutput).await?;
        let selected = SelectedAudioOutputPayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_selected_audio_output(selected)
    }

    async fn trusted_subtitle_presentation_context(
        &self,
    ) -> Result<TrustedSubtitlePresentationContext, MediaBrokerError> {
        let response = self
            .request(BrokerCommand::TrustedSubtitlePresentationContext)
            .await?;
        let payload =
            TrustedSubtitlePresentationContextPayload::decode(response.payload.as_slice())
                .map_err(|_| MediaBrokerError::Malformed)?;
        decode_trusted_subtitle_presentation_context(payload)
    }

    async fn enumerate_audio_inputs(&self) -> Result<AudioInputSnapshot, MediaBrokerError> {
        let response = self.request(BrokerCommand::EnumerateAudioInputs).await?;
        let payload = AudioInputSnapshotPayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_audio_input_snapshot(payload)
    }

    async fn select_audio_input(
        &self,
        selection: &AudioInputSelection,
    ) -> Result<SelectedAudioInput, MediaBrokerError> {
        let payload = encode_audio_input_selection(selection)?;
        let response = self
            .request_with_payload(BrokerCommand::SelectAudioInput, payload.encode_to_vec())
            .await?;
        let selected = SelectedAudioInputPayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_selected_audio_input(selected)
    }

    async fn selected_audio_input(&self) -> Result<SelectedAudioInput, MediaBrokerError> {
        let response = self.request(BrokerCommand::SelectedAudioInput).await?;
        let selected = SelectedAudioInputPayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_selected_audio_input(selected)
    }

    async fn query_ptt_activation_state(&self) -> Result<PttActivationState, MediaBrokerError> {
        let response = self.request(BrokerCommand::QueryPttActivationState).await?;
        let payload = PttActivationStatePayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_ptt_activation_state(payload)
    }

    async fn begin_manual_actor_picker(
        &self,
        request: &NativeManualActorPickerRequestV1,
    ) -> Result<NativeManualActorPickerReceiptV1, MediaBrokerError> {
        validate_manual_actor_picker_request(request)?;
        let response = self
            .request_with_payload(
                BrokerCommand::ManualActorPicker,
                manual_actor_picker_command_payload(ManualActorPickerActionPayload::Begin, request)
                    .encode_to_vec(),
            )
            .await?;
        decode_manual_actor_picker_receipt(response.payload.as_slice(), request)
    }

    async fn query_manual_actor_picker(
        &self,
        request: &NativeManualActorPickerRequestV1,
    ) -> Result<NativeManualActorPickerReceiptV1, MediaBrokerError> {
        validate_manual_actor_picker_request(request)?;
        let response = self
            .request_with_payload(
                BrokerCommand::ManualActorPicker,
                manual_actor_picker_command_payload(ManualActorPickerActionPayload::Poll, request)
                    .encode_to_vec(),
            )
            .await?;
        decode_manual_actor_picker_receipt(response.payload.as_slice(), request)
    }

    async fn cancel_manual_actor_picker(
        &self,
        request: &NativeManualActorPickerRequestV1,
    ) -> Result<NativeManualActorPickerReceiptV1, MediaBrokerError> {
        validate_manual_actor_picker_request(request)?;
        let response = self
            .request_with_payload(
                BrokerCommand::ManualActorPicker,
                manual_actor_picker_command_payload(
                    ManualActorPickerActionPayload::Cancel,
                    request,
                )
                .encode_to_vec(),
            )
            .await?;
        decode_manual_actor_picker_receipt(response.payload.as_slice(), request)
    }

    async fn allocate_audio_input_rehearsal(
        &self,
        request: &AllocateAudioInputRehearsalPayload,
    ) -> Result<AudioInputRehearsalLease, MediaBrokerError> {
        let response = self
            .request_with_payload(
                BrokerCommand::AllocateAudioInputRehearsal,
                request.encode_to_vec(),
            )
            .await?;
        let payload = AudioInputRehearsalLeasePayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_audio_input_rehearsal_lease(payload, request)
    }

    async fn cancel_audio_input_rehearsal(
        &self,
        stream_id: &str,
        generation: u64,
    ) -> Result<(), MediaBrokerError> {
        self.request_with_payload(
            BrokerCommand::CancelAudioInputRehearsal,
            CancelAudioInputRehearsalPayload {
                stream_id: stream_id.to_owned(),
                generation,
            }
            .encode_to_vec(),
        )
        .await
        .map(|_| ())
    }

    async fn query_visual_audio_envelope(
        &self,
        query: &VisualAudioEnvelopeQuery,
    ) -> Result<VisualAudioEnvelope, MediaBrokerError> {
        validate_visual_audio_envelope_query(query)?;
        let response = self
            .request_with_payload(
                BrokerCommand::QueryVisualAudioEnvelope,
                QueryVisualAudioEnvelopePayload {
                    session_id: query.session_id.clone(),
                    turn_id: query.turn_id.clone(),
                    generation: query.generation,
                    stream_id: query.stream_id.clone(),
                    segment_id: query.segment_id.clone(),
                }
                .encode_to_vec(),
            )
            .await?;
        let payload = VisualAudioEnvelopePayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_visual_audio_envelope(payload, query)
    }

    async fn allocate_playback_stream(
        &self,
        request: &AllocatePlaybackStreamPayload,
    ) -> Result<AudioPlaybackLease, MediaBrokerError> {
        let response = self
            .request_with_payload(
                BrokerCommand::AllocatePlaybackStream,
                request.encode_to_vec(),
            )
            .await?;
        let payload = PlaybackLeasePayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_playback_lease(payload, request)
    }

    async fn cancel_playback(&self) -> Result<(), MediaBrokerError> {
        self.request(BrokerCommand::CancelPlayback)
            .await
            .map(|_| ())
    }

    async fn allocate_visual_source(
        &self,
        worker: &VisualWorkerIdentity,
        track: &VisualTrackBinding,
    ) -> Result<VisualSourceLease, MediaBrokerError> {
        let wire = AllocateVisualSourcePayload {
            worker_process_id: worker.process_id,
            worker_process_creation_time: worker.process_creation_time,
            worker_executable_name: worker.executable_name.clone(),
            actor_id: track.actor_id,
            track_id: track.track_id,
            track_epoch: track.track_epoch,
        };
        let response = self
            .request_with_payload(BrokerCommand::AllocateVisualSource, wire.encode_to_vec())
            .await?;
        let payload = VisualSourceLeasePayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_visual_source_lease(payload, worker, track)
    }

    async fn release_visual_source(
        &self,
        lease: &VisualSourceLease,
    ) -> Result<(), MediaBrokerError> {
        self.request_with_payload(
            BrokerCommand::ReleaseVisualSource,
            ReleaseVisualSourcePayload {
                worker_process_id: lease.worker.process_id,
                lease_nonce_high: lease.lease_nonce_high,
                lease_nonce_low: lease.lease_nonce_low,
            }
            .encode_to_vec(),
        )
        .await
        .map(|_| ())
    }

    async fn allocate_identity_frame(
        &self,
        worker: &IdentityWorkerIdentity,
        crop: IdentityCrop,
    ) -> Result<IdentityFrameLease, MediaBrokerError> {
        let expected_session_id = self
            .0
            .lock()
            .map_err(|_| MediaBrokerError::State)?
            .session_id
            .clone();
        let request = AllocateIdentityFramePayload {
            worker_process_id: worker.process_id,
            worker_process_creation_time: worker.process_creation_time,
            worker_executable_name: worker.executable_name.clone(),
            crop_left: crop.left,
            crop_top: crop.top,
            crop_right: crop.right,
            crop_bottom: crop.bottom,
        };
        let response = self
            .request_with_payload(
                BrokerCommand::AllocateIdentityFrame,
                request.encode_to_vec(),
            )
            .await?;
        let payload = IdentityFrameLeasePayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_identity_frame_lease(payload, worker, crop, &expected_session_id)
    }

    async fn release_identity_frame(
        &self,
        lease: &IdentityFrameLease,
    ) -> Result<(), MediaBrokerError> {
        self.request_with_payload(
            BrokerCommand::ReleaseIdentityFrame,
            ReleaseIdentityFramePayload {
                worker_process_id: lease.worker.process_id,
                lease_id: lease.lease_id().to_owned(),
                lease_nonce: lease.lease_nonce().to_owned(),
            }
            .encode_to_vec(),
        )
        .await
        .map(|_| ())
    }

    async fn submit_visual_occlusion(
        &self,
        lease: &VisualSourceLease,
        evidence: &BrokerOcclusionEvidence,
    ) -> Result<(), MediaBrokerError> {
        let payload = SubmitOcclusionPayload {
            face_confidence: evidence.face_confidence,
            landmark_confidence: evidence.landmark_confidence,
            visibility_ratio: evidence.visibility_ratio,
            mouth_occluded: evidence.mouth_occluded,
            measured_qpc: evidence.measured_qpc,
            source_frame_sequence: lease.source_frame_sequence,
            source_device_generation: lease.source_device_generation,
            track_epoch: lease.track.track_epoch,
            source_frame_qpc: lease.source_frame_qpc,
            actor_id: lease.track.actor_id,
            track_id: lease.track.track_id,
            source_geometry_epoch: lease.source_geometry_epoch,
        };
        self.request_with_payload(BrokerCommand::SubmitOcclusion, payload.encode_to_vec())
            .await
            .map(|_| ())
    }

    async fn allocate_identity_reference_import(
        &self,
        request: &IdentityReferenceImportRequest,
    ) -> Result<IdentityReferenceImportLease, MediaBrokerError> {
        validate_identity_reference_import_request(request)?;
        let expected_session_id = self
            .0
            .lock()
            .map_err(|_| MediaBrokerError::State)?
            .session_id
            .clone();
        let wire = AllocateIdentityReferenceImportPayload {
            worker_process_id: request.worker.process_id,
            worker_process_creation_time: request.worker.process_creation_time,
            worker_executable_name: request.worker.executable_name.clone(),
            picker_consent_token: request.picker_consent_token.clone(),
            game_profile_id: request.game_profile_id.clone(),
            character_id: request.character_id.clone(),
            subject_id: request.subject_id.clone(),
            reference_id: request.reference_id.clone(),
            subject_display_name: request.subject_display_name.clone(),
            source_class: match request.source_class {
                IdentityReferenceSourceClass::UserPrivate => {
                    IdentityReferenceSourceClassPayload::UserPrivate as i32
                }
                IdentityReferenceSourceClass::OriginalSynthetic => {
                    IdentityReferenceSourceClassPayload::OriginalSynthetic as i32
                }
            },
            owner_user_id: request.owner_user_id.clone().unwrap_or_default(),
            original_work_license: request.original_work_license.clone().unwrap_or_default(),
            explicit_user_consent: request.explicit_user_consent,
            local_only: request.local_only,
            imported_at_unix_ms: request.imported_at_unix_ms,
        };
        let response = self
            .request_with_payload(
                BrokerCommand::AllocateIdentityReferenceImport,
                wire.encode_to_vec(),
            )
            .await?;
        let payload = IdentityReferenceImportLeasePayload::decode(response.payload.as_slice())
            .map_err(|_| MediaBrokerError::Malformed)?;
        decode_identity_reference_import_lease(payload, request, &expected_session_id)
    }

    async fn release_identity_reference_import(
        &self,
        lease: &IdentityReferenceImportLease,
    ) -> Result<(), MediaBrokerError> {
        self.request_with_payload(
            BrokerCommand::ReleaseIdentityReferenceImport,
            ReleaseIdentityReferenceImportPayload {
                worker_process_id: lease.worker.process_id,
                lease_id: lease.lease_id().to_owned(),
                lease_nonce: lease.lease_nonce().to_owned(),
            }
            .encode_to_vec(),
        )
        .await
        .map(|_| ())
    }

    async fn submit_visual_patch(
        &self,
        lease: &VisualSourceLease,
        residual: &BrokerResidualProposal,
    ) -> Result<BrokerPresentationReceipt, MediaBrokerError> {
        let (nonce, session_id) = {
            let connection = self.0.lock().map_err(|_| MediaBrokerError::State)?;
            (connection.nonce.to_vec(), connection.session_id.clone())
        };
        let payload = SubmitPatchPayload {
            source_frame_sequence: lease.source_frame_sequence,
            cancellation_generation: lease.cancellation_generation,
            left: residual.left,
            top: residual.top,
            right: residual.right,
            bottom: residual.bottom,
            confidence: residual.confidence,
            produced_qpc: residual.produced_qpc,
            shared_texture: Some(SharedTexturePayload {
                schema_version: 1,
                session_nonce: nonce,
                session_id,
                lease_nonce_high: residual.lease_nonce_high,
                lease_nonce_low: residual.lease_nonce_low,
                worker_process_id: residual.worker.process_id,
                worker_process_creation_time: residual.worker.process_creation_time,
                worker_executable_name: residual.worker.executable_name.clone(),
                source_process_handle_value: residual.worker_handle_value,
                adapter_luid: residual.adapter_luid,
                keyed_mutex_acquire_key: residual.keyed_mutex_acquire_key,
                keyed_mutex_release_key: residual.keyed_mutex_release_key,
                width: residual.width,
                height: residual.height,
                stride_bytes: residual.stride_bytes,
                dxgi_format: residual.dxgi_format,
                alpha_mode: residual.alpha_mode,
                expires_qpc: residual.expires_qpc,
            }),
            source_device_generation: lease.source_device_generation,
            source_frame_qpc: lease.source_frame_qpc,
            track_epoch: lease.track.track_epoch,
            actor_id: lease.track.actor_id,
            track_id: lease.track.track_id,
            source_geometry_epoch: lease.source_geometry_epoch,
        };
        let response = self
            .request_with_payload(BrokerCommand::SubmitPatch, payload.encode_to_vec())
            .await?;
        decode_presentation_receipt(&response, lease)
    }

    async fn select_target(
        &self,
        native_window: u64,
        process_id: u32,
        executable_basename: &str,
    ) -> Result<(), MediaBrokerError> {
        let payload = SelectTargetPayload {
            native_window,
            expected_process_id: process_id,
            allowed_process_names: vec![executable_basename.to_owned()],
        }
        .encode_to_vec();
        self.request_with_payload(BrokerCommand::SelectTarget, payload)
            .await
            .map(|_| ())
    }

    async fn clear_target(&self) -> Result<(), MediaBrokerError> {
        self.request(BrokerCommand::ClearTarget).await.map(|_| ())
    }

    async fn capture_evidence(&self) -> Result<NativeCaptureEvidence, MediaBrokerError> {
        let response = self.request(BrokerCommand::CaptureEvidence).await?;
        decode_native_capture_evidence(&response.payload)
    }

    async fn shutdown(&self) -> Result<(), MediaBrokerError> {
        self.request(BrokerCommand::Shutdown).await.map(|_| ())
    }

    async fn request(&self, command: BrokerCommand) -> Result<BrokerResponse, MediaBrokerError> {
        self.request_with_payload(command, Vec::new()).await
    }

    async fn request_with_payload(
        &self,
        command: BrokerCommand,
        payload: Vec<u8>,
    ) -> Result<BrokerResponse, MediaBrokerError> {
        let connection = Arc::clone(&self.0);
        let task = tauri::async_runtime::spawn_blocking(move || {
            request_blocking(&connection, command, payload)
        });
        tokio::time::timeout(REQUEST_TIMEOUT, task)
            .await
            .map_err(|_| MediaBrokerError::Timeout)?
            .map_err(|_| MediaBrokerError::Connection)?
    }
}

fn request_blocking(
    connection: &Mutex<BrokerConnection>,
    command: BrokerCommand,
    payload: Vec<u8>,
) -> Result<BrokerResponse, MediaBrokerError> {
    if payload.len().saturating_add(4) > MAX_FRAME_BYTES {
        return Err(MediaBrokerError::Payload);
    }
    let mut connection = connection.lock().map_err(|_| MediaBrokerError::State)?;
    let sequence = connection.next_sequence;
    connection.next_sequence = connection.next_sequence.saturating_add(1);
    let (now, frequency) = qpc_now();
    let envelope = BrokerEnvelope {
        version: PROTOCOL_VERSION,
        launch_nonce: connection.nonce.to_vec(),
        session_id: connection.session_id.clone(),
        sequence,
        deadline_qpc: now.saturating_add(frequency.saturating_mul(5)),
        cancellation_generation: connection.cancellation_generation,
        command: command as i32,
        payload,
    };
    let body = envelope.encode_to_vec();
    write_frame(&mut connection.stream, &body)?;
    let frame = read_frame(&mut connection.stream)?;
    let response =
        BrokerResponse::decode(frame.as_slice()).map_err(|_| MediaBrokerError::Malformed)?;
    if response.version != PROTOCOL_VERSION || response.response_to_sequence != sequence {
        return Err(MediaBrokerError::Malformed);
    }
    let status = response.status();
    connection.cancellation_generation = reconcile_response_generation(
        command,
        connection.cancellation_generation,
        response.cancellation_generation,
        status,
    )?;
    if status != BrokerStatus::Ok && command != BrokerCommand::SubmitPatch {
        return Err(MediaBrokerError::Remote(status));
    }
    Ok(response)
}

fn reconcile_response_generation(
    command: BrokerCommand,
    current: u64,
    response: u64,
    status: BrokerStatus,
) -> Result<u64, MediaBrokerError> {
    let mutating = command.advances_cancellation_generation_on_success();
    if !mutating {
        return (response == current)
            .then_some(current)
            .ok_or(MediaBrokerError::Malformed);
    }

    let next = current.checked_add(1).ok_or(MediaBrokerError::Malformed)?;
    if response != current && response != next {
        return Err(MediaBrokerError::Malformed);
    }
    if status == BrokerStatus::Ok {
        // Newer brokers return the post-command generation. The original V1
        // SelectTarget/ClearTarget response was assembled before dispatch and
        // still carried `current`; both represent the same guaranteed one-step
        // transition after a successful target mutation.
        Ok(next)
    } else {
        // A rejected command may fail before mutation. Only adopt a transition
        // when the broker explicitly reports it.
        Ok(response)
    }
}

#[derive(Debug, Clone, Copy)]
struct BrokerHealth {
    broker_state: u32,
}

#[derive(Clone, Debug)]
pub struct MediaBrokerLaunchConfig {
    pub executable: PathBuf,
    pub development_fixture_allowed: bool,
    pub audio_output_selection_path: PathBuf,
    #[cfg(debug_assertions)]
    pub debug_synthetic_metadata_path: PathBuf,
}

impl MediaBrokerLaunchConfig {
    pub fn from_application(
        development_fixture_allowed: bool,
        app_config_directory: &Path,
    ) -> Result<Self, MediaBrokerError> {
        #[cfg(not(debug_assertions))]
        let _ = app_config_directory;
        let executable = std::env::current_exe()
            .map_err(|_| MediaBrokerError::InvalidBundle)?
            .parent()
            .ok_or(MediaBrokerError::InvalidBundle)?
            .join(BROKER_FILE_NAME);
        Ok(Self {
            executable,
            development_fixture_allowed,
            audio_output_selection_path: app_config_directory
                .join(AUDIO_OUTPUT_SELECTION_FILE_NAME),
            #[cfg(debug_assertions)]
            debug_synthetic_metadata_path: app_config_directory
                .join(DEBUG_SYNTHETIC_METADATA_FILE_NAME),
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedAudioOutputSelection {
    schema_version: u32,
    selection: AudioOutputSelection,
}

fn load_audio_output_selection(
    path: &Path,
) -> Result<Option<AudioOutputSelection>, MediaBrokerError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(MediaBrokerError::AudioOutputPersistence(error.to_string())),
    };
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err(MediaBrokerError::AudioOutputPersistence(
            "selection file is empty or exceeds 4 KiB".into(),
        ));
    }
    let persisted: PersistedAudioOutputSelection = serde_json::from_slice(&bytes)
        .map_err(|error| MediaBrokerError::AudioOutputPersistence(error.to_string()))?;
    if persisted.schema_version != 1 {
        return Err(MediaBrokerError::AudioOutputPersistence(
            "selection schema version is unsupported".into(),
        ));
    }
    encode_audio_output_selection(&persisted.selection)?;
    Ok(Some(persisted.selection))
}

fn persist_audio_output_selection(
    path: &Path,
    selection: &AudioOutputSelection,
) -> Result<(), MediaBrokerError> {
    encode_audio_output_selection(selection)?;
    let parent = path.parent().ok_or_else(|| {
        MediaBrokerError::AudioOutputPersistence("selection path has no parent".into())
    })?;
    std::fs::create_dir_all(parent)
        .map_err(|error| MediaBrokerError::AudioOutputPersistence(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&PersistedAudioOutputSelection {
        schema_version: 1,
        selection: selection.clone(),
    })
    .map_err(|error| MediaBrokerError::AudioOutputPersistence(error.to_string()))?;
    if bytes.len() > 4096 {
        return Err(MediaBrokerError::AudioOutputPersistence(
            "selection serialization exceeds 4 KiB".into(),
        ));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| MediaBrokerError::AudioOutputPersistence(error.to_string()))?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| MediaBrokerError::AudioOutputPersistence(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| MediaBrokerError::AudioOutputPersistence(error.error.to_string()))?;
    Ok(())
}

fn restore_audio_output_selection(
    path: &Path,
    selection: Option<&AudioOutputSelection>,
) -> Result<(), MediaBrokerError> {
    if let Some(selection) = selection {
        return persist_audio_output_selection(path, selection);
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(MediaBrokerError::AudioOutputPersistence(error.to_string())),
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedAudioInputSelection {
    schema_version: u32,
    selection: AudioInputSelection,
}

fn audio_input_selection_path(output_selection_path: &Path) -> Result<PathBuf, MediaBrokerError> {
    output_selection_path
        .parent()
        .map(|parent| parent.join(AUDIO_INPUT_SELECTION_FILE_NAME))
        .ok_or_else(|| {
            MediaBrokerError::AudioInputPersistence(
                "audio output selection path has no configuration parent".into(),
            )
        })
}

fn load_audio_input_selection(
    path: &Path,
) -> Result<Option<AudioInputSelection>, MediaBrokerError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(MediaBrokerError::AudioInputPersistence(error.to_string())),
    };
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err(MediaBrokerError::AudioInputPersistence(
            "selection file is empty or exceeds 4 KiB".into(),
        ));
    }
    let persisted: PersistedAudioInputSelection = serde_json::from_slice(&bytes)
        .map_err(|error| MediaBrokerError::AudioInputPersistence(error.to_string()))?;
    if persisted.schema_version != 1 {
        return Err(MediaBrokerError::AudioInputPersistence(
            "selection schema version is unsupported".into(),
        ));
    }
    encode_audio_input_selection(&persisted.selection)?;
    Ok(Some(persisted.selection))
}

fn persist_audio_input_selection(
    path: &Path,
    selection: &AudioInputSelection,
) -> Result<(), MediaBrokerError> {
    encode_audio_input_selection(selection)?;
    let parent = path.parent().ok_or_else(|| {
        MediaBrokerError::AudioInputPersistence("selection path has no parent".into())
    })?;
    std::fs::create_dir_all(parent)
        .map_err(|error| MediaBrokerError::AudioInputPersistence(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&PersistedAudioInputSelection {
        schema_version: 1,
        selection: selection.clone(),
    })
    .map_err(|error| MediaBrokerError::AudioInputPersistence(error.to_string()))?;
    if bytes.len() > 4096 {
        return Err(MediaBrokerError::AudioInputPersistence(
            "selection serialization exceeds 4 KiB".into(),
        ));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| MediaBrokerError::AudioInputPersistence(error.to_string()))?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| MediaBrokerError::AudioInputPersistence(error.to_string()))?;
    temporary
        .persist(path)
        .map_err(|error| MediaBrokerError::AudioInputPersistence(error.error.to_string()))?;
    Ok(())
}

fn restore_audio_input_selection(
    path: &Path,
    selection: Option<&AudioInputSelection>,
) -> Result<(), MediaBrokerError> {
    if let Some(selection) = selection {
        return persist_audio_input_selection(path, selection);
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(MediaBrokerError::AudioInputPersistence(error.to_string())),
    }
}

fn rollback_status<T, E: std::fmt::Display>(result: &Result<T, E>) -> String {
    match result {
        Ok(_) => "ok".into(),
        Err(error) => error.to_string(),
    }
}

fn selection_rollback_status<T: PartialEq, E: std::fmt::Display>(
    result: &Result<T, E>,
    expected: &T,
) -> String {
    match result {
        Ok(actual) if actual == expected => "ok".into(),
        Ok(_) => "mismatched-success".into(),
        Err(error) => error.to_string(),
    }
}

#[derive(Debug)]
struct ManagedBroker {
    child: BrokerChild,
    client: BrokerClient,
    pid: u32,
}

#[derive(Debug)]
struct BrokerSupervisorState {
    connection: RuntimeConnectionState,
    detail: String,
    process_id: Option<u32>,
    restart_count: u32,
    recent_failures: VecDeque<Instant>,
    broker_state: Option<u32>,
    diagnostics: Option<MediaBrokerDiagnostics>,
    #[cfg(debug_assertions)]
    debug_synthetic_target: Option<DebugSyntheticTargetIdentity>,
}

#[derive(Clone)]
pub struct MediaBrokerSupervisor {
    config: Arc<MediaBrokerLaunchConfig>,
    parent_job: RuntimeSupervisor,
    state: Arc<Mutex<BrokerSupervisorState>>,
    managed: Arc<tokio::sync::Mutex<Option<ManagedBroker>>>,
    startup_gate: Arc<tokio::sync::Mutex<()>>,
    shutdown_token: CancellationToken,
    #[cfg(debug_assertions)]
    debug_synthetic_capture_gate: Arc<tokio::sync::Mutex<()>>,
}

impl std::fmt::Debug for MediaBrokerSupervisor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaBrokerSupervisor")
            .field("health", &self.health())
            .finish_non_exhaustive()
    }
}

impl MediaBrokerSupervisor {
    pub fn new(config: MediaBrokerLaunchConfig, parent_job: RuntimeSupervisor) -> Self {
        let exists = config.executable.is_file();
        let (connection, detail) = if exists {
            (
                RuntimeConnectionState::Cold,
                "Bundled media broker is ready to start.",
            )
        } else if config.development_fixture_allowed {
            (
                RuntimeConnectionState::DevelopmentFixture,
                "Media broker is skipped in this unbundled source-development build.",
            )
        } else {
            (
                RuntimeConnectionState::Unavailable,
                "Required bundled media broker is missing.",
            )
        };
        Self {
            config: Arc::new(config),
            parent_job,
            state: Arc::new(Mutex::new(BrokerSupervisorState {
                connection,
                detail: detail.into(),
                process_id: None,
                restart_count: 0,
                recent_failures: VecDeque::new(),
                broker_state: None,
                diagnostics: None,
                #[cfg(debug_assertions)]
                debug_synthetic_target: None,
            })),
            managed: Arc::new(tokio::sync::Mutex::new(None)),
            startup_gate: Arc::new(tokio::sync::Mutex::new(())),
            shutdown_token: CancellationToken::new(),
            #[cfg(debug_assertions)]
            debug_synthetic_capture_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn health(&self) -> MediaBrokerHealthSnapshot {
        let Ok(state) = self.state.lock() else {
            return unavailable_health("Media broker supervisor state is unavailable.");
        };
        let diagnostics = state.diagnostics.as_ref();
        MediaBrokerHealthSnapshot {
            state: state.connection,
            connected: state.connection == RuntimeConnectionState::Ready,
            process_id: state.process_id,
            restart_count: state.restart_count,
            recent_failure_count: state.recent_failures.len() as u32,
            protocol_version: (state.connection == RuntimeConnectionState::Ready)
                .then_some(PROTOCOL_VERSION),
            fixture_only: state.connection == RuntimeConnectionState::DevelopmentFixture,
            broker_state: state.broker_state.map(broker_state_name).map(str::to_owned),
            capture_available: diagnostics.is_some_and(|value| value.capture_backend != "none"),
            overlay_available: diagnostics.is_some_and(|value| value.overlay_backend != "none"),
            capture_audio_available: diagnostics
                .is_some_and(|value| matches!(value.capture_audio.as_str(), "ready" | "capturing")),
            render_audio_available: diagnostics
                .is_some_and(|value| matches!(value.render_audio.as_str(), "ready" | "playing")),
            detail: state.detail.clone(),
        }
    }

    pub fn start_background(&self) {
        if self.health().state == RuntimeConnectionState::DevelopmentFixture {
            return;
        }
        let supervisor = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                if supervisor.shutdown_token.is_cancelled() {
                    break;
                }
                let _ = supervisor.ensure_ready().await;
                tokio::select! {
                    _ = supervisor.shutdown_token.cancelled() => break,
                    _ = tokio::time::sleep(HEARTBEAT_INTERVAL) => {}
                }
            }
        });
    }

    async fn ensure_ready(&self) -> Result<BrokerClient, MediaBrokerError> {
        let health = self.health();
        if health.state == RuntimeConnectionState::DevelopmentFixture {
            return Err(MediaBrokerError::DevelopmentFixture);
        }
        if health.state == RuntimeConnectionState::Quarantined {
            return Err(MediaBrokerError::Quarantined);
        }
        let _startup = self.startup_gate.lock().await;
        if let Some(client) = self.live_client().await? {
            return Ok(client);
        }
        self.apply_backoff().await?;
        self.set_connection(
            RuntimeConnectionState::Starting,
            "Starting authenticated media broker.",
        );
        match self.launch().await {
            Ok(managed) => {
                let client = managed.client.clone();
                let pid = managed.pid;
                *self.managed.lock().await = Some(managed);
                self.set_ready(pid);
                Ok(client)
            }
            Err(error) => {
                self.record_failure(error.to_string()).await;
                Err(error)
            }
        }
    }

    pub async fn refresh_health(&self) -> Result<MediaBrokerHealthSnapshot, MediaBrokerError> {
        let client = self.ensure_ready().await?;
        let health = client.health().await?;
        let diagnostics = client.diagnostics().await?;
        if let Ok(mut state) = self.state.lock() {
            state.broker_state = Some(health.broker_state);
            state.diagnostics = Some(diagnostics);
        }
        Ok(self.health())
    }

    pub async fn diagnostics(&self) -> Result<MediaBrokerDiagnostics, MediaBrokerError> {
        let client = self.ensure_ready().await?;
        let diagnostics = client.diagnostics().await?;
        if let Ok(mut state) = self.state.lock() {
            state.diagnostics = Some(diagnostics.clone());
        }
        Ok(diagnostics)
    }

    /// Native-only exact capture evidence for product coordinators. The
    /// WebView never receives this lease authority or uses display pixels as a
    /// substitute for exact-window WGC evidence.
    pub(crate) async fn native_capture_evidence(
        &self,
    ) -> Result<NativeCaptureEvidence, MediaBrokerError> {
        let client = self.ensure_ready().await?;
        client.capture_evidence().await
    }

    /// Native-only display and capture attestation for trusted subtitle
    /// placement. This must never be sourced from or round-tripped through the
    /// WebView.
    pub(crate) async fn trusted_subtitle_presentation_context(
        &self,
    ) -> Result<TrustedSubtitlePresentationContext, MediaBrokerError> {
        self.ensure_ready()
            .await?
            .trusted_subtitle_presentation_context()
            .await
    }

    /// Native-only, read-only causal playback envelope. This contains only
    /// bounded RMS/peak coefficients published after WASAPI accepted a buffer;
    /// it never exposes PCM, shared memory, pipe names, or tokens.
    pub(crate) async fn query_visual_audio_envelope(
        &self,
        query: VisualAudioEnvelopeQuery,
    ) -> Result<VisualAudioEnvelope, MediaBrokerError> {
        self.ensure_ready()
            .await?
            .query_visual_audio_envelope(&query)
            .await
    }

    pub async fn enumerate_audio_outputs(&self) -> Result<AudioOutputSnapshot, MediaBrokerError> {
        self.ensure_ready().await?.enumerate_audio_outputs().await
    }

    pub async fn selected_audio_output(
        &self,
    ) -> Result<Option<SelectedAudioOutput>, MediaBrokerError> {
        let Some(selection) =
            load_audio_output_selection(&self.config.audio_output_selection_path)?
        else {
            return Ok(None);
        };
        let client = self.ensure_ready().await?;
        let current = client.selected_audio_output().await?;
        let selected = if current.selection == selection {
            current
        } else {
            client.select_audio_output(&selection).await?
        };
        Ok(Some(selected))
    }

    pub async fn select_audio_output(
        &self,
        selection: AudioOutputSelection,
    ) -> Result<SelectedAudioOutput, MediaBrokerError> {
        encode_audio_output_selection(&selection)?;
        let previous = load_audio_output_selection(&self.config.audio_output_selection_path)?;
        let client = self.ensure_ready().await?;
        let previous_broker = client.selected_audio_output().await?;
        // Commit durable intent first, but still capture and restore the exact
        // live broker selection on every non-exact result. This remains safe
        // even if a future native implementation mutates before returning Err.
        persist_audio_output_selection(&self.config.audio_output_selection_path, &selection)?;
        match client.select_audio_output(&selection).await {
            Ok(selected) if selected.selection == selection => Ok(selected),
            Ok(_) => {
                let broker_rollback = client.select_audio_output(&previous_broker.selection).await;
                let durable_rollback = restore_audio_output_selection(
                    &self.config.audio_output_selection_path,
                    previous.as_ref(),
                );
                if !matches!(&broker_rollback, Ok(actual) if actual == &previous_broker)
                    || durable_rollback.is_err()
                {
                    return Err(MediaBrokerError::AudioOutputPersistence(format!(
                        "mismatched broker selection; broker rollback={}; durable rollback={}",
                        selection_rollback_status(&broker_rollback, &previous_broker),
                        rollback_status(&durable_rollback)
                    )));
                }
                Err(MediaBrokerError::Malformed)
            }
            Err(activation) => {
                let broker_rollback = client.select_audio_output(&previous_broker.selection).await;
                let durable_rollback = restore_audio_output_selection(
                    &self.config.audio_output_selection_path,
                    previous.as_ref(),
                );
                if !matches!(&broker_rollback, Ok(actual) if actual == &previous_broker)
                    || durable_rollback.is_err()
                {
                    return Err(MediaBrokerError::AudioOutputPersistence(format!(
                        "broker activation failed ({activation}); broker rollback={}; durable rollback={}",
                        selection_rollback_status(&broker_rollback, &previous_broker),
                        rollback_status(&durable_rollback)
                    )));
                }
                Err(activation)
            }
        }
    }

    pub async fn enumerate_audio_inputs(&self) -> Result<AudioInputSnapshot, MediaBrokerError> {
        self.ensure_ready().await?.enumerate_audio_inputs().await
    }

    pub async fn selected_audio_input(
        &self,
    ) -> Result<Option<SelectedAudioInput>, MediaBrokerError> {
        let path = audio_input_selection_path(&self.config.audio_output_selection_path)?;
        let Some(selection) = load_audio_input_selection(&path)? else {
            return Ok(None);
        };
        let client = self.ensure_ready().await?;
        let current = client.selected_audio_input().await?;
        let selected = if current.selection == selection {
            current
        } else {
            client.select_audio_input(&selection).await?
        };
        Ok(Some(selected))
    }

    /// Native-only read-only PTT snapshot used to arm against a baseline and
    /// poll at no more than 40 Hz for a strictly newer physical press. No pipe,
    /// token, PCM, or allocation authority crosses this boundary.
    pub(crate) async fn query_ptt_activation_state(
        &self,
    ) -> Result<PttActivationState, MediaBrokerError> {
        self.ensure_ready()
            .await?
            .query_ptt_activation_state()
            .await
    }

    pub(crate) async fn begin_manual_actor_picker(
        &self,
        request: &NativeManualActorPickerRequestV1,
    ) -> Result<NativeManualActorPickerReceiptV1, MediaBrokerError> {
        self.ensure_ready()
            .await?
            .begin_manual_actor_picker(request)
            .await
    }

    pub(crate) async fn query_manual_actor_picker(
        &self,
        request: &NativeManualActorPickerRequestV1,
    ) -> Result<NativeManualActorPickerReceiptV1, MediaBrokerError> {
        self.ensure_ready()
            .await?
            .query_manual_actor_picker(request)
            .await
    }

    pub(crate) async fn cancel_manual_actor_picker(
        &self,
        request: &NativeManualActorPickerRequestV1,
    ) -> Result<NativeManualActorPickerReceiptV1, MediaBrokerError> {
        self.ensure_ready()
            .await?
            .cancel_manual_actor_picker(request)
            .await
    }

    pub async fn select_audio_input(
        &self,
        selection: AudioInputSelection,
    ) -> Result<SelectedAudioInput, MediaBrokerError> {
        encode_audio_input_selection(&selection)?;
        let path = audio_input_selection_path(&self.config.audio_output_selection_path)?;
        let previous = load_audio_input_selection(&path)?;
        let client = self.ensure_ready().await?;
        let previous_broker = client.selected_audio_input().await?;
        persist_audio_input_selection(&path, &selection)?;
        match client.select_audio_input(&selection).await {
            Ok(selected) if selected.selection == selection => Ok(selected),
            Ok(_) => {
                let broker_rollback = client.select_audio_input(&previous_broker.selection).await;
                let durable_rollback = restore_audio_input_selection(&path, previous.as_ref());
                if !matches!(&broker_rollback, Ok(actual) if actual == &previous_broker)
                    || durable_rollback.is_err()
                {
                    return Err(MediaBrokerError::AudioInputPersistence(format!(
                        "mismatched broker selection; broker rollback={}; durable rollback={}",
                        selection_rollback_status(&broker_rollback, &previous_broker),
                        rollback_status(&durable_rollback)
                    )));
                }
                Err(MediaBrokerError::Malformed)
            }
            Err(activation) => {
                let broker_rollback = client.select_audio_input(&previous_broker.selection).await;
                let durable_rollback = restore_audio_input_selection(&path, previous.as_ref());
                if !matches!(&broker_rollback, Ok(actual) if actual == &previous_broker)
                    || durable_rollback.is_err()
                {
                    return Err(MediaBrokerError::AudioInputPersistence(format!(
                        "broker activation failed ({activation}); broker rollback={}; durable rollback={}",
                        selection_rollback_status(&broker_rollback, &previous_broker),
                        rollback_status(&durable_rollback)
                    )));
                }
                Err(activation)
            }
        }
    }

    /// Allocates one native-only microphone stream for the supervised runtime
    /// child. The pipe endpoint and token must not be exposed to the WebView.
    pub(crate) async fn allocate_audio_input_rehearsal(
        &self,
        request: AudioInputRehearsalRequest,
    ) -> Result<AudioInputRehearsalLease, MediaBrokerError> {
        validate_audio_input_rehearsal_request(&request)?;
        self.parent_job
            .ensure_ready()
            .await
            .map_err(|error| MediaBrokerError::Runtime(error.to_string()))?;
        let producer_process_id = self
            .parent_job
            .health()
            .process_id
            .filter(|process_id| *process_id != 0)
            .ok_or(MediaBrokerError::RuntimeUnavailable)?;
        let path = audio_input_selection_path(&self.config.audio_output_selection_path)?;
        let selection = load_audio_input_selection(&path)?
            .ok_or(MediaBrokerError::AudioInputSelectionRequired)?;
        let client = self.ensure_ready().await?;
        let selected = client.select_audio_input(&selection).await?;
        let wire = AllocateAudioInputRehearsalPayload {
            session_id: request.session_id,
            turn_id: request.turn_id,
            generation: request.generation,
            duration_ms: request.duration_ms,
            sample_rate: request.sample_rate,
            channels: u32::from(request.channels),
            max_frames: request.max_frames,
            expected_producer_process_id: producer_process_id,
            activation_source: match request.activation_source {
                InputActivationSource::PushToTalk => {
                    InputActivationSourcePayload::PushToTalk as i32
                }
                InputActivationSource::ExplicitRehearsal => {
                    return Err(MediaBrokerError::InvalidAudioInputRequest);
                }
            },
        };
        let lease = client.allocate_audio_input_rehearsal(&wire).await?;
        if lease.input_endpoint_id != selected.resolved.endpoint_id
            || lease.input_endpoint_generation != selected.resolved.generation
        {
            let _ = client
                .cancel_audio_input_rehearsal(&lease.stream_id, lease.generation)
                .await;
            return Err(MediaBrokerError::Malformed);
        }
        Ok(lease)
    }

    pub(crate) async fn cancel_audio_input_rehearsal(
        &self,
        stream_id: &str,
        generation: u64,
    ) -> Result<(), MediaBrokerError> {
        validate_bounded_identifier(stream_id, 128)?;
        if generation == 0 {
            return Err(MediaBrokerError::InvalidAudioInputRequest);
        }
        self.ensure_ready()
            .await?
            .cancel_audio_input_rehearsal(stream_id, generation)
            .await
    }

    /// Allocates a bounded pool of one-time producer leases for one pinned turn.
    /// The expected producer PID comes only from the supervised runtime child;
    /// WebView input cannot choose or override it.
    pub async fn allocate_playback_pool(
        &self,
        request: AudioPlaybackPoolRequest,
    ) -> Result<Vec<AudioPlaybackLease>, MediaBrokerError> {
        validate_playback_pool_request(&request)?;
        self.parent_job
            .ensure_ready()
            .await
            .map_err(|error| MediaBrokerError::Runtime(error.to_string()))?;
        let producer_process_id = self
            .parent_job
            .health()
            .process_id
            .filter(|process_id| *process_id != 0)
            .ok_or(MediaBrokerError::RuntimeUnavailable)?;
        let client = self.ensure_ready().await?;
        let selection = load_audio_output_selection(&self.config.audio_output_selection_path)?
            .ok_or(MediaBrokerError::AudioOutputSelectionRequired)?;
        let selected_output = client.select_audio_output(&selection).await?;
        let wire = AllocatePlaybackStreamPayload {
            session_id: request.session_id,
            turn_id: request.turn_id,
            generation: request.generation,
            sample_rate: request.sample_rate,
            channels: u32::from(request.channels),
            max_frames: request.max_frames_per_lease,
            expected_producer_process_id: producer_process_id,
        };
        let mut leases = Vec::with_capacity(request.lease_count);
        for _ in 0..request.lease_count {
            match client.allocate_playback_stream(&wire).await {
                Ok(lease)
                    if lease.output_endpoint_id == selected_output.resolved.endpoint_id
                        && lease.output_endpoint_generation
                            == selected_output.resolved.generation =>
                {
                    leases.push(lease)
                }
                Ok(_) => {
                    let _ = client.cancel_playback().await;
                    return Err(MediaBrokerError::Malformed);
                }
                Err(error) => {
                    // Pool allocation is atomic from the turn's perspective.
                    // Revoke every endpoint already issued before surfacing the
                    // failure; the runtime never receives a partial lease set.
                    let _ = client.cancel_playback().await;
                    return Err(error);
                }
            }
        }
        Ok(leases)
    }

    pub async fn cancel_playback(&self) -> Result<(), MediaBrokerError> {
        let client = self.ensure_ready().await?;
        client.cancel_playback().await
    }

    pub async fn allocate_visual_source(
        &self,
        worker: &VisualWorkerIdentity,
        track: &VisualTrackBinding,
    ) -> Result<VisualSourceLease, MediaBrokerError> {
        validate_visual_identity(worker, track)?;
        self.ensure_ready()
            .await?
            .allocate_visual_source(worker, track)
            .await
    }

    pub async fn release_visual_source(
        &self,
        lease: &VisualSourceLease,
    ) -> Result<(), MediaBrokerError> {
        self.ensure_ready()
            .await?
            .release_visual_source(lease)
            .await
    }

    pub async fn allocate_identity_frame(
        &self,
        worker: &IdentityWorkerIdentity,
        crop: IdentityCrop,
    ) -> Result<IdentityFrameLease, MediaBrokerError> {
        validate_identity_request(worker, crop)?;
        self.ensure_ready()
            .await?
            .allocate_identity_frame(worker, crop)
            .await
    }

    pub(crate) async fn allocate_identity_reference_import(
        &self,
        request: &IdentityReferenceImportRequest,
    ) -> Result<IdentityReferenceImportLease, MediaBrokerError> {
        self.ensure_ready()
            .await?
            .allocate_identity_reference_import(request)
            .await
    }

    pub(crate) async fn release_identity_reference_import(
        &self,
        lease: &IdentityReferenceImportLease,
    ) -> Result<(), MediaBrokerError> {
        self.ensure_ready()
            .await?
            .release_identity_reference_import(lease)
            .await
    }

    pub async fn release_identity_frame(
        &self,
        lease: &IdentityFrameLease,
    ) -> Result<(), MediaBrokerError> {
        self.ensure_ready()
            .await?
            .release_identity_frame(lease)
            .await
    }

    pub async fn submit_visual_occlusion(
        &self,
        lease: &VisualSourceLease,
        evidence: &BrokerOcclusionEvidence,
    ) -> Result<(), MediaBrokerError> {
        validate_occlusion_evidence(evidence)?;
        self.ensure_ready()
            .await?
            .submit_visual_occlusion(lease, evidence)
            .await
    }

    pub async fn submit_visual_patch(
        &self,
        lease: &VisualSourceLease,
        residual: &BrokerResidualProposal,
    ) -> Result<BrokerPresentationReceipt, MediaBrokerError> {
        validate_residual_proposal(lease, residual)?;
        self.ensure_ready()
            .await?
            .submit_visual_patch(lease, residual)
            .await
    }

    pub(crate) async fn clear_bound_target(&self) -> Result<(), MediaBrokerError> {
        let client = self.ensure_ready().await?;
        client.clear_target().await
    }

    /// Selects the task-owned synthetic replay window in debug builds only.
    ///
    /// The native broker still performs its complete same-user/session,
    /// anti-cheat, HWND/PID, and executable-name policy inspection. This
    /// control-plane gate additionally prevents the debug command from being
    /// reused to capture an arbitrary game or application.
    #[cfg(debug_assertions)]
    pub async fn debug_select_synthetic_replay_capture_target(
        &self,
    ) -> Result<DebugSyntheticReplayCaptureSnapshot, MediaBrokerError> {
        let identity = read_debug_synthetic_target(&self.config.debug_synthetic_metadata_path)?;
        let _operation = self.debug_synthetic_capture_gate.lock().await;
        {
            let state = self.state.lock().map_err(|_| MediaBrokerError::State)?;
            if state
                .debug_synthetic_target
                .as_ref()
                .is_some_and(|selected| selected != &identity)
            {
                return Err(MediaBrokerError::DebugSyntheticTargetMismatch);
            }
        }

        let client = self.ensure_ready().await?;
        if let Err(error) = client
            .select_target(
                identity.native_window,
                identity.process_id,
                &identity.executable_basename,
            )
            .await
        {
            if let Ok(mut state) = self.state.lock() {
                state.debug_synthetic_target = None;
                state.diagnostics = None;
            }
            return Err(error);
        }
        if let Ok(mut state) = self.state.lock() {
            state.debug_synthetic_target = Some(identity.clone());
        }
        let diagnostics = client.diagnostics().await?;
        if let Ok(mut state) = self.state.lock() {
            state.diagnostics = Some(diagnostics.clone());
        }
        // Selection and capture proof are separate operations. The caller can
        // poll the diagnostics command after the first captured frame.
        Ok(debug_synthetic_capture_snapshot(
            identity,
            diagnostics,
            None,
        ))
    }

    #[cfg(debug_assertions)]
    pub async fn debug_clear_synthetic_replay_capture_target(
        &self,
    ) -> Result<DebugSyntheticReplayCaptureSnapshot, MediaBrokerError> {
        let _operation = self.debug_synthetic_capture_gate.lock().await;
        let identity = self.selected_debug_synthetic_target()?;
        let client = self.ensure_ready().await?;
        client.clear_target().await?;
        if let Ok(mut state) = self.state.lock() {
            state.debug_synthetic_target = None;
            state.diagnostics = None;
        }
        let diagnostics = client.diagnostics().await?;
        if let Ok(mut state) = self.state.lock() {
            state.diagnostics = Some(diagnostics.clone());
        }
        Ok(debug_synthetic_capture_snapshot(
            identity,
            diagnostics,
            None,
        ))
    }

    #[cfg(debug_assertions)]
    pub async fn debug_synthetic_replay_capture_diagnostics(
        &self,
    ) -> Result<DebugSyntheticReplayCaptureSnapshot, MediaBrokerError> {
        let _operation = self.debug_synthetic_capture_gate.lock().await;
        let identity = self.selected_debug_synthetic_target()?;
        let client = self.ensure_ready().await?;
        let diagnostics = client.diagnostics().await?;
        let evidence = client.capture_evidence().await?;
        validate_debug_capture_evidence(&identity, &evidence)?;
        Ok(debug_synthetic_capture_snapshot(
            identity,
            diagnostics,
            Some(evidence),
        ))
    }

    #[cfg(debug_assertions)]
    fn selected_debug_synthetic_target(
        &self,
    ) -> Result<DebugSyntheticTargetIdentity, MediaBrokerError> {
        let state = self.state.lock().map_err(|_| MediaBrokerError::State)?;
        state
            .debug_synthetic_target
            .clone()
            .ok_or(MediaBrokerError::DebugSyntheticTargetMismatch)
    }

    /// Native-only capability check for the project-owned review target. This
    /// state is populated only after the bounded metadata document proves the
    /// exact executable, embedded portrait digest, and static source mouth.
    #[cfg(debug_assertions)]
    pub(crate) fn project_owned_review_target_selected(&self) -> bool {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.debug_synthetic_target.clone())
            .is_some_and(|target| {
                target.executable_basename == DEBUG_SYNTHETIC_TARGET_BASENAME
                    && target.portrait_sha256 == DEBUG_SYNTHETIC_PORTRAIT_SHA256
            })
    }

    pub async fn shutdown(&self) {
        self.shutdown_token.cancel();
        self.set_connection(
            RuntimeConnectionState::ShuttingDown,
            "Media broker is shutting down.",
        );
        if let Some(mut managed) = self.managed.lock().await.take() {
            let _ = managed.client.shutdown().await;
            if tokio::time::timeout(SHUTDOWN_TIMEOUT, managed.child.wait())
                .await
                .is_err()
            {
                let _ = managed.child.kill().await;
                let _ = managed.child.wait().await;
            }
        }
        self.set_connection(RuntimeConnectionState::Stopped, "Media broker is stopped.");
    }

    async fn live_client(&self) -> Result<Option<BrokerClient>, MediaBrokerError> {
        let (client, exited) = {
            let mut managed = self.managed.lock().await;
            let Some(managed) = managed.as_mut() else {
                return Ok(None);
            };
            let exited = managed
                .child
                .try_wait()
                .map_err(|_| MediaBrokerError::Process)?;
            (managed.client.clone(), exited.is_some())
        };
        if exited {
            self.record_failure("Media broker exited unexpectedly.".into())
                .await;
            return Ok(None);
        }
        match client.health().await {
            Ok(health) => {
                if let Ok(mut state) = self.state.lock() {
                    state.broker_state = Some(health.broker_state);
                }
                Ok(Some(client))
            }
            Err(error) => {
                self.record_failure(error.to_string()).await;
                Ok(None)
            }
        }
    }

    async fn launch(&self) -> Result<ManagedBroker, MediaBrokerError> {
        let executable = validate_fixed_broker(&self.config.executable)?;
        let parent_pid = std::process::id();
        let mut nonce = [0_u8; 32];
        getrandom::fill(&mut nonce).map_err(|_| MediaBrokerError::Random)?;
        let session_id = format!("media-{}", uuid::Uuid::new_v4().simple());
        let nonce_hex = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let args = [
            format!("--parent-pid={parent_pid}"),
            format!("--session={session_id}"),
            format!("--nonce={nonce_hex}"),
        ];
        let child = spawn_broker_process(&executable, &args, &self.parent_job)?;
        let pid = child.id();
        let pipe = format!(r"\\.\pipe\npc-media-broker-{session_id}");
        let stream = connect_broker_pipe(&pipe).await?;
        let client = BrokerClient::new(stream, nonce, session_id);
        client.health().await?;
        Ok(ManagedBroker { child, client, pid })
    }

    async fn record_failure(&self, detail: String) {
        if let Some(mut managed) = self.managed.lock().await.take() {
            let _ = managed.child.kill().await;
            let _ = managed.child.wait().await;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let now = Instant::now();
        state.recent_failures.push_back(now);
        while state
            .recent_failures
            .front()
            .is_some_and(|failure| now.duration_since(*failure) > FAILURE_WINDOW)
        {
            state.recent_failures.pop_front();
        }
        state.process_id = None;
        state.diagnostics = None;
        #[cfg(debug_assertions)]
        {
            state.debug_synthetic_target = None;
        }
        if state.recent_failures.len() >= MAX_FAILURES {
            state.connection = RuntimeConnectionState::Quarantined;
            state.detail = "Media broker quarantined after three failures in sixty seconds; runtime conversation remains available with audio/subtitle fallbacks.".into();
        } else {
            state.connection = RuntimeConnectionState::RestartBackoff;
            state.restart_count = state.restart_count.saturating_add(1);
            state.detail = detail;
        }
    }

    async fn apply_backoff(&self) -> Result<(), MediaBrokerError> {
        let failures = self
            .state
            .lock()
            .map_err(|_| MediaBrokerError::State)?
            .recent_failures
            .len();
        if failures >= MAX_FAILURES {
            return Err(MediaBrokerError::Quarantined);
        }
        let delay = match failures {
            0 => Duration::ZERO,
            1 => Duration::from_millis(250),
            _ => Duration::from_secs(1),
        };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        Ok(())
    }

    fn set_ready(&self, pid: u32) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.connection = RuntimeConnectionState::Ready;
        state.process_id = Some(pid);
        state.detail = "Authenticated media broker control channel is ready.".into();
    }

    fn set_connection(&self, connection: RuntimeConnectionState, detail: &str) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.connection = connection;
        state.detail = detail.into();
        if connection != RuntimeConnectionState::Ready {
            state.process_id = None;
        }
    }
}

#[cfg(debug_assertions)]
fn validate_debug_synthetic_target(
    native_window: u64,
    process_id: u32,
    executable_basename: &str,
    portrait_sha256: &str,
    qualification_mode: DebugSyntheticQualificationMode,
) -> Result<DebugSyntheticTargetIdentity, MediaBrokerError> {
    if native_window == 0 || usize::try_from(native_window).is_err() {
        return Err(MediaBrokerError::DebugSyntheticTargetInvalid);
    }
    if process_id == 0
        || executable_basename != DEBUG_SYNTHETIC_TARGET_BASENAME
        || portrait_sha256 != DEBUG_SYNTHETIC_PORTRAIT_SHA256
    {
        return Err(MediaBrokerError::DebugSyntheticTargetInvalid);
    }
    Ok(DebugSyntheticTargetIdentity {
        native_window,
        process_id,
        executable_basename: executable_basename.into(),
        portrait_sha256: portrait_sha256.into(),
        qualification_mode,
    })
}

#[cfg(debug_assertions)]
fn read_debug_synthetic_target(
    path: &Path,
) -> Result<DebugSyntheticTargetIdentity, MediaBrokerError> {
    if path.file_name().and_then(|value| value.to_str()) != Some(DEBUG_SYNTHETIC_METADATA_FILE_NAME)
    {
        return Err(MediaBrokerError::DebugSyntheticMetadataInvalid);
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| MediaBrokerError::DebugSyntheticMetadataUnavailable)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > DEBUG_SYNTHETIC_METADATA_MAX_BYTES
    {
        return Err(MediaBrokerError::DebugSyntheticMetadataInvalid);
    }
    let bytes = std::fs::read(path).map_err(|_| MediaBrokerError::DebugSyntheticMetadataInvalid)?;
    if bytes.len() as u64 > DEBUG_SYNTHETIC_METADATA_MAX_BYTES {
        return Err(MediaBrokerError::DebugSyntheticMetadataInvalid);
    }
    decode_debug_synthetic_target_metadata(&bytes)
}

#[cfg(debug_assertions)]
fn decode_debug_synthetic_target_metadata(
    bytes: &[u8],
) -> Result<DebugSyntheticTargetIdentity, MediaBrokerError> {
    let metadata: DebugSyntheticTargetMetadata = serde_json::from_slice(bytes)
        .map_err(|_| MediaBrokerError::DebugSyntheticMetadataInvalid)?;
    if metadata.fixture_kind != "synthetic-original-video-replay"
        || metadata.state != "playing"
        || metadata.decoded_frames == 0
        || metadata.pid != metadata.process_id
        || metadata.hwnd != metadata.window_handle
        || metadata.window_handle <= 0
        || metadata.exe_basename != metadata.executable_basename
        || metadata.portrait_sha256 != DEBUG_SYNTHETIC_PORTRAIT_SHA256
    {
        return Err(MediaBrokerError::DebugSyntheticMetadataInvalid);
    }
    let qualification_mode = match metadata.schema_version {
        1 if metadata.visual_source == DEBUG_SYNTHETIC_VISUAL_SOURCE
            && metadata.source_mouth_motion == Some(false) =>
        {
            DebugSyntheticQualificationMode::StaticCompatibility
        }
        2 if metadata.visual_source == DEBUG_SYNTHETIC_MOVING_VISUAL_SOURCE
            && metadata.source_sequence_sha256.as_deref()
                == Some(DEBUG_SYNTHETIC_SOURCE_SEQUENCE_SHA256)
            && metadata.source_frame_count == Some(42)
            && metadata.source_frame_width == Some(960)
            && metadata.source_frame_height == Some(720)
            && metadata.source_frame_rate == Some(30)
            && metadata.source_frame_motion == Some(true)
            && metadata.source_actor_motion == Some(false)
            && metadata.rendered_actor_motion == Some(true)
            && metadata.rendered_blink_motion == Some(true)
            && metadata.rendered_breathing_motion == Some(true)
            && metadata.source_mouth_articulation == Some(false)
            && metadata.product_lip_sync == Some(false)
            && metadata.visual_mode.as_deref() == Some("moving-source-controlled-idle-v2")
            && metadata.content_frame_index.is_some_and(|index| index < 42)
            && metadata
                .content_frame_sha256
                .as_deref()
                .is_some_and(valid_lower_sha256) =>
        {
            DebugSyntheticQualificationMode::MovingSourceControlledIdle
        }
        _ => return Err(MediaBrokerError::DebugSyntheticMetadataInvalid),
    };
    validate_debug_synthetic_target(
        metadata.window_handle as u64,
        metadata.process_id,
        &metadata.executable_basename,
        &metadata.portrait_sha256,
        qualification_mode,
    )
}

#[cfg(debug_assertions)]
fn debug_synthetic_capture_snapshot(
    identity: DebugSyntheticTargetIdentity,
    diagnostics: MediaBrokerDiagnostics,
    capture_evidence: Option<NativeCaptureEvidence>,
) -> DebugSyntheticReplayCaptureSnapshot {
    DebugSyntheticReplayCaptureSnapshot {
        target_process_id: identity.process_id,
        target_window_handle: identity.native_window,
        target_executable_basename: identity.executable_basename,
        fixture_motion_mode: identity.qualification_mode.as_str().into(),
        diagnostics,
        capture_evidence,
    }
}

#[cfg(debug_assertions)]
fn validate_debug_capture_evidence(
    identity: &DebugSyntheticTargetIdentity,
    evidence: &NativeCaptureEvidence,
) -> Result<(), MediaBrokerError> {
    if evidence.selected_process_id != identity.process_id
        || evidence.selected_window_handle != identity.native_window
        || !evidence
            .selected_executable_name
            .eq_ignore_ascii_case(&identity.executable_basename)
    {
        return Err(MediaBrokerError::DebugSyntheticTargetMismatch);
    }
    Ok(())
}

fn validate_fixed_broker(path: &Path) -> Result<PathBuf, MediaBrokerError> {
    if !path.is_absolute()
        || path.file_name().and_then(|value| value.to_str()) != Some(BROKER_FILE_NAME)
    {
        return Err(MediaBrokerError::InvalidBundle);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| MediaBrokerError::Missing)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(MediaBrokerError::InvalidBundle);
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| MediaBrokerError::InvalidBundle)?;
    let parent = path
        .parent()
        .ok_or(MediaBrokerError::InvalidBundle)?
        .canonicalize()
        .map_err(|_| MediaBrokerError::InvalidBundle)?;
    if canonical.parent() != Some(parent.as_path()) {
        return Err(MediaBrokerError::InvalidBundle);
    }
    Ok(canonical)
}

#[cfg(windows)]
#[derive(Debug)]
struct BrokerChild {
    handle: windows_sys::Win32::Foundation::HANDLE,
    process_id: u32,
}

#[cfg(windows)]
// SAFETY: the process HANDLE is a kernel reference usable across threads; this
// wrapper owns it until Drop and does not expose borrowed process memory.
unsafe impl Send for BrokerChild {}

#[cfg(windows)]
impl BrokerChild {
    fn id(&self) -> u32 {
        self.process_id
    }

    fn try_wait(&mut self) -> Result<Option<u32>, MediaBrokerError> {
        use windows_sys::Win32::Foundation::STILL_ACTIVE;
        use windows_sys::Win32::System::Threading::GetExitCodeProcess;
        let mut code = 0_u32;
        // SAFETY: handle is a live process handle and code is writable.
        if unsafe { GetExitCodeProcess(self.handle, &mut code) } == 0 {
            return Err(MediaBrokerError::Process);
        }
        if code == STILL_ACTIVE as u32 {
            Ok(None)
        } else {
            Ok(Some(code))
        }
    }

    async fn wait(&mut self) -> Result<u32, MediaBrokerError> {
        loop {
            if let Some(code) = self.try_wait()? {
                return Ok(code);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn kill(&mut self) -> Result<(), MediaBrokerError> {
        use windows_sys::Win32::System::Threading::TerminateProcess;
        // SAFETY: handle is a live process handle owned by this wrapper.
        if unsafe { TerminateProcess(self.handle, 1) } == 0 {
            return Err(MediaBrokerError::Process);
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for BrokerChild {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::TerminateProcess;
        if matches!(self.try_wait(), Ok(None)) {
            // SAFETY: handle is live; kill-on-drop prevents an orphan if async
            // teardown could not complete.
            unsafe { TerminateProcess(self.handle, 1) };
        }
        // SAFETY: this wrapper exclusively owns the process handle.
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(windows)]
fn spawn_broker_process(
    executable: &Path,
    args: &[String],
    parent_job: &RuntimeSupervisor,
) -> Result<BrokerChild, MediaBrokerError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, ResumeThread, TerminateProcess, CREATE_NO_WINDOW, CREATE_SUSPENDED,
        CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
    };

    let application: Vec<u16> = executable.as_os_str().encode_wide().chain([0]).collect();
    let executable_text = executable.to_string_lossy().replace('"', "");
    let mut command_line = format!("\"{executable_text}\" {}", args.join(" "))
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let current_directory: Vec<u16> = executable
        .parent()
        .ok_or(MediaBrokerError::InvalidBundle)?
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect();
    let environment = minimal_environment_block();
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..STARTUPINFOW::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: all UTF-16 buffers are NUL-terminated and remain live. No handles
    // are inherited. The process starts suspended so it cannot inspect launch
    // context before joining the parent-owned kill-on-close Job Object.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            current_directory.as_ptr(),
            &startup,
            &mut process,
        )
    };
    if created == 0 || process.hProcess.is_null() || process.hThread.is_null() {
        return Err(MediaBrokerError::Process);
    }
    if let Err(error) = parent_job.assign_raw_process_to_parent_job(process.hProcess as usize) {
        // SAFETY: both handles are exclusively owned on this failure path.
        unsafe {
            TerminateProcess(process.hProcess, 1);
            CloseHandle(process.hThread);
            CloseHandle(process.hProcess);
        }
        return Err(MediaBrokerError::Job(error.to_string()));
    }
    // SAFETY: the primary thread is suspended exactly once and its live handle
    // remains valid until closed immediately below.
    let resumed = unsafe { ResumeThread(process.hThread) };
    // SAFETY: the primary thread handle is no longer required by the parent.
    unsafe { CloseHandle(process.hThread) };
    if resumed == u32::MAX {
        // SAFETY: process handle is live and exclusively owned.
        unsafe {
            TerminateProcess(process.hProcess, 1);
            CloseHandle(process.hProcess);
        }
        return Err(MediaBrokerError::Process);
    }
    Ok(BrokerChild {
        handle: process.hProcess,
        process_id: process.dwProcessId,
    })
}

#[cfg(windows)]
fn minimal_environment_block() -> Vec<u16> {
    // Explicit allowlist: enough Windows/user profile context for COM, WinRT,
    // WASAPI, and D3D initialization, while excluding arbitrary variables that
    // may contain provider credentials, tokens, or developer secrets.
    let mut entries = [
        "ALLUSERSPROFILE",
        "APPDATA",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "CommonProgramW6432",
        "COMPUTERNAME",
        "ComSpec",
        "DriverData",
        "HOMEDRIVE",
        "HOMEPATH",
        "LOCALAPPDATA",
        "NUMBER_OF_PROCESSORS",
        "OS",
        "Path",
        "PATHEXT",
        "PROCESSOR_ARCHITECTURE",
        "PROCESSOR_IDENTIFIER",
        "PROCESSOR_LEVEL",
        "PROCESSOR_REVISION",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "PUBLIC",
        "SystemDrive",
        "SystemRoot",
        "TEMP",
        "TMP",
        "USERDOMAIN",
        "USERNAME",
        "USERPROFILE",
        "WINDIR",
    ]
    .into_iter()
    .filter_map(|key| {
        std::env::var_os(key).map(|value| format!("{key}={}", value.to_string_lossy()))
    })
    .collect::<Vec<_>>();
    entries.sort_by_key(|value| value.to_ascii_uppercase());
    let mut block = Vec::new();
    for entry in entries {
        block.extend(entry.encode_utf16());
        block.push(0);
    }
    block.push(0);
    block
}

#[cfg(not(windows))]
#[derive(Debug)]
struct BrokerChild(tokio::process::Child);

#[cfg(not(windows))]
impl BrokerChild {
    fn id(&self) -> u32 {
        self.0.id().unwrap_or(0)
    }
    fn try_wait(&mut self) -> Result<Option<u32>, MediaBrokerError> {
        self.0
            .try_wait()
            .map(|status| status.map(|value| value.code().unwrap_or(1) as u32))
            .map_err(|_| MediaBrokerError::Process)
    }
    async fn wait(&mut self) -> Result<u32, MediaBrokerError> {
        self.0
            .wait()
            .await
            .map(|status| status.code().unwrap_or(1) as u32)
            .map_err(|_| MediaBrokerError::Process)
    }
    async fn kill(&mut self) -> Result<(), MediaBrokerError> {
        self.0.kill().await.map_err(|_| MediaBrokerError::Process)
    }
}

#[cfg(not(windows))]
fn spawn_broker_process(
    executable: &Path,
    args: &[String],
    _parent_job: &RuntimeSupervisor,
) -> Result<BrokerChild, MediaBrokerError> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
        .spawn()
        .map(BrokerChild)
        .map_err(|_| MediaBrokerError::Process)
}

#[cfg(windows)]
async fn connect_broker_pipe(pipe: &str) -> Result<BrokerStream, MediaBrokerError> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
    let pipe: Vec<u16> = std::ffi::OsStr::new(pipe)
        .encode_wide()
        .chain([0])
        .collect();
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    loop {
        // SAFETY: pipe is a fixed, NUL-terminated per-launch name. No handle is
        // inherited and OPEN_EXISTING cannot create a filesystem object.
        let handle = unsafe {
            CreateFileW(
                pipe.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            // SAFETY: the successful CreateFileW handle is transferred exactly
            // once to File, which closes it on Drop.
            return Ok(unsafe { std::fs::File::from_raw_handle(handle) });
        }
        // SAFETY: GetLastError has no preconditions and immediately follows the
        // failed CreateFileW call on this thread.
        let last_error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if tokio::time::Instant::now() >= deadline {
            return Err(MediaBrokerError::ConnectionCode(last_error));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(not(windows))]
async fn connect_broker_pipe(_pipe: &str) -> Result<BrokerStream, MediaBrokerError> {
    Err(MediaBrokerError::DevelopmentFixture)
}

fn write_frame<W: Write>(writer: &mut W, body: &[u8]) -> Result<(), MediaBrokerError> {
    if body.is_empty() || body.len().saturating_add(4) > MAX_FRAME_BYTES {
        return Err(MediaBrokerError::Payload);
    }
    let length = u32::try_from(body.len()).map_err(|_| MediaBrokerError::Payload)?;
    writer
        .write_all(&length.to_le_bytes())
        .map_err(|_| MediaBrokerError::Connection)?;
    writer
        .write_all(body)
        .map_err(|_| MediaBrokerError::Connection)?;
    writer.flush().map_err(|_| MediaBrokerError::Connection)
}

fn read_frame<R: Read>(reader: &mut R) -> Result<Vec<u8>, MediaBrokerError> {
    let mut prefix = [0_u8; 4];
    reader
        .read_exact(&mut prefix)
        .map_err(|_| MediaBrokerError::Connection)?;
    let length = u32::from_le_bytes(prefix) as usize;
    if length == 0 || length.saturating_add(4) > MAX_FRAME_BYTES {
        return Err(MediaBrokerError::Payload);
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| MediaBrokerError::Connection)?;
    Ok(body)
}

fn validate_visual_identity(
    worker: &VisualWorkerIdentity,
    track: &VisualTrackBinding,
) -> Result<(), MediaBrokerError> {
    if worker.process_id == 0
        || worker.process_creation_time == 0
        || worker.executable_name.is_empty()
        || worker.executable_name.len() > 260
        || worker
            .executable_name
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\'))
        || track.actor_id == 0
        || track.track_id == 0
        || track.track_epoch == 0
    {
        return Err(MediaBrokerError::InvalidVisualRequest);
    }
    Ok(())
}

fn manual_actor_picker_command_payload(
    action: ManualActorPickerActionPayload,
    request: &NativeManualActorPickerRequestV1,
) -> ManualActorPickerCommandPayload {
    let begin = action == ManualActorPickerActionPayload::Begin;
    ManualActorPickerCommandPayload {
        action: action as i32,
        request_id: request.request_id.clone(),
        source_device_generation: if begin {
            request.source_device_generation
        } else {
            0
        },
        source_geometry_epoch: if begin {
            request.source_geometry_epoch
        } else {
            0
        },
        source_frame_sequence: if begin {
            request.source_frame_sequence
        } else {
            0
        },
        source_frame_qpc: if begin { request.source_frame_qpc } else { 0 },
        timeout_ms: if begin { request.timeout_ms } else { 0 },
        candidates: if begin {
            request
                .candidates
                .iter()
                .map(|candidate| ManualActorCandidatePayload {
                    actor_id: candidate.actor_id,
                    track_id: candidate.track_id,
                    track_epoch: candidate.track_epoch,
                    left: candidate.left,
                    top: candidate.top,
                    right: candidate.right,
                    bottom: candidate.bottom,
                })
                .collect()
        } else {
            Vec::new()
        },
    }
}

pub(crate) fn manual_actor_candidate_set_sha256(
    candidates: &[NativeManualActorCandidateV1],
) -> String {
    let mut canonical = Vec::with_capacity(candidates.len().saturating_mul(56));
    for candidate in candidates {
        for value in [
            candidate.actor_id,
            candidate.track_id,
            candidate.track_epoch,
            candidate.left.to_bits(),
            candidate.top.to_bits(),
            candidate.right.to_bits(),
            candidate.bottom.to_bits(),
        ] {
            canonical.extend_from_slice(&value.to_le_bytes());
        }
    }
    hex::encode(Sha256::digest(canonical))
}

fn validate_manual_actor_picker_request(
    request: &NativeManualActorPickerRequestV1,
) -> Result<(), MediaBrokerError> {
    validate_bounded_identifier(&request.request_id, 128)?;
    validate_bounded_identifier(&request.visual_pack_id, 128)?;
    validate_bounded_identifier(&request.game_profile_id, 128)?;
    if !valid_lower_sha256(&request.visual_pack_admission_sha256)
        || request.capture_session_id.is_empty()
        || request.capture_session_id.len() > 64
        || request.capture_session_id.contains('\0')
        || request.cancellation_generation == 0
        || request.selected_process_id == 0
        || request.selected_window_handle == 0
        || request.selected_executable_name.is_empty()
        || request.selected_executable_name.len() > 260
        || request.selected_executable_name.contains(['/', '\\'])
        || request.source_device_generation == 0
        || request.source_geometry_epoch == 0
        || request.source_frame_sequence == 0
        || request.source_frame_qpc == 0
        || request.qpc_frequency == 0
        || request.captured_at_unix_ms == 0
        || request.expires_at_unix_ms <= request.captured_at_unix_ms
        || request.source_width == 0
        || request.source_height == 0
        || !(500..=15_000).contains(&request.timeout_ms)
        || request.candidates.is_empty()
        || request.candidates.len() > 64
    {
        return Err(MediaBrokerError::InvalidManualActorPickerRequest);
    }
    let mut actors = std::collections::BTreeSet::new();
    let mut tracks = std::collections::BTreeSet::new();
    for candidate in &request.candidates {
        if candidate.actor_id == 0
            || candidate.track_id == 0
            || candidate.track_epoch == 0
            || !actors.insert(candidate.actor_id)
            || !tracks.insert((candidate.track_id, candidate.track_epoch))
            || ![
                candidate.left,
                candidate.top,
                candidate.right,
                candidate.bottom,
            ]
            .into_iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
            || candidate.right - candidate.left < 0.005
            || candidate.bottom - candidate.top < 0.005
        {
            return Err(MediaBrokerError::InvalidManualActorPickerRequest);
        }
    }
    Ok(())
}

fn decode_manual_actor_picker_receipt(
    bytes: &[u8],
    request: &NativeManualActorPickerRequestV1,
) -> Result<NativeManualActorPickerReceiptV1, MediaBrokerError> {
    let payload =
        ManualActorPickerReceiptPayload::decode(bytes).map_err(|_| MediaBrokerError::Malformed)?;
    let status = match payload.status {
        1 => NativeManualActorPickerStatusV1::Pending,
        2 => NativeManualActorPickerStatusV1::Selected,
        3 => NativeManualActorPickerStatusV1::Cancelled,
        4 => NativeManualActorPickerStatusV1::TimedOut,
        5 => NativeManualActorPickerStatusV1::TargetLost,
        6 => NativeManualActorPickerStatusV1::TargetResized,
        7 => NativeManualActorPickerStatusV1::DpiChanged,
        8 => NativeManualActorPickerStatusV1::DeviceChanged,
        9 => NativeManualActorPickerStatusV1::CaptureChanged,
        10 => NativeManualActorPickerStatusV1::ClickOutsideDetectedRoi,
        11 => NativeManualActorPickerStatusV1::AmbiguousDetectedRoi,
        12 => NativeManualActorPickerStatusV1::UntrustedPointerInput,
        13 => NativeManualActorPickerStatusV1::OverlayUnavailable,
        14 => NativeManualActorPickerStatusV1::InternalError,
        _ => return Err(MediaBrokerError::Malformed),
    };
    let selected = status == NativeManualActorPickerStatusV1::Selected;
    let click_terminal = selected
        || matches!(
            status,
            NativeManualActorPickerStatusV1::ClickOutsideDetectedRoi
                | NativeManualActorPickerStatusV1::AmbiguousDetectedRoi
                | NativeManualActorPickerStatusV1::UntrustedPointerInput
        );
    let candidate_digest = manual_actor_candidate_set_sha256(&request.candidates);
    if payload.schema_version != 1
        || payload.request_id != request.request_id
        || (payload.receipt_nonce_high == 0 && payload.receipt_nonce_low == 0)
        || payload.capture_session_id != request.capture_session_id
        || payload.cancellation_generation != request.cancellation_generation
        || payload.selected_process_id != request.selected_process_id
        || payload.selected_window_handle != request.selected_window_handle
        || payload.selected_executable_name != request.selected_executable_name
        || payload.source_device_generation != request.source_device_generation
        || payload.source_geometry_epoch != request.source_geometry_epoch
        || payload.source_frame_sequence != request.source_frame_sequence
        || payload.source_frame_qpc != request.source_frame_qpc
        || payload.candidate_count as usize != request.candidates.len()
        || payload.candidate_set_sha256 != candidate_digest
        || payload.began_qpc == 0
        || payload.attested_at_qpc < payload.began_qpc
        || payload.qpc_frequency != request.qpc_frequency
        || payload.pointer_kind > 3
        || !payload.frozen_wgc_frame_verified
        || !payload.overlay_capture_excluded
        || !payload.overlay_nonactivating
        || !payload.pixels_withheld_from_webview
        || !payload.coordinates_withheld_from_webview
        || (selected
            != (payload.selected_actor_id != 0
                && payload.selected_track_id != 0
                && payload.selected_track_epoch != 0))
        || (!selected
            && (payload.selected_actor_id != 0
                || payload.selected_track_id != 0
                || payload.selected_track_epoch != 0))
        || (selected && (!payload.single_hardware_pointer_click || payload.pointer_kind == 0))
        || (click_terminal != (payload.clicked_qpc != 0))
        || (click_terminal && payload.clicked_qpc < payload.began_qpc)
        || (payload.single_hardware_pointer_click && !selected)
    {
        return Err(MediaBrokerError::Malformed);
    }
    if selected
        && !request.candidates.iter().any(|candidate| {
            candidate.actor_id == payload.selected_actor_id
                && candidate.track_id == payload.selected_track_id
                && candidate.track_epoch == payload.selected_track_epoch
        })
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(NativeManualActorPickerReceiptV1 {
        schema_version: payload.schema_version,
        request_id: payload.request_id,
        status,
        receipt_nonce_high: payload.receipt_nonce_high,
        receipt_nonce_low: payload.receipt_nonce_low,
        capture_session_id: payload.capture_session_id,
        cancellation_generation: payload.cancellation_generation,
        selected_process_id: payload.selected_process_id,
        selected_window_handle: payload.selected_window_handle,
        selected_executable_name: payload.selected_executable_name,
        source_device_generation: payload.source_device_generation,
        source_geometry_epoch: payload.source_geometry_epoch,
        source_frame_sequence: payload.source_frame_sequence,
        source_frame_qpc: payload.source_frame_qpc,
        selected_actor_id: payload.selected_actor_id,
        selected_track_id: payload.selected_track_id,
        selected_track_epoch: payload.selected_track_epoch,
        candidate_count: payload.candidate_count,
        candidate_set_sha256: payload.candidate_set_sha256,
        began_qpc: payload.began_qpc,
        clicked_qpc: payload.clicked_qpc,
        attested_at_qpc: payload.attested_at_qpc,
        qpc_frequency: payload.qpc_frequency,
        pointer_kind: payload.pointer_kind,
        frozen_wgc_frame_verified: payload.frozen_wgc_frame_verified,
        overlay_capture_excluded: payload.overlay_capture_excluded,
        overlay_nonactivating: payload.overlay_nonactivating,
        single_hardware_pointer_click: payload.single_hardware_pointer_click,
        pixels_withheld_from_webview: payload.pixels_withheld_from_webview,
        coordinates_withheld_from_webview: payload.coordinates_withheld_from_webview,
        receipt_sha256: hex::encode(Sha256::digest(bytes)),
    })
}

fn validate_identity_request(
    worker: &IdentityWorkerIdentity,
    crop: IdentityCrop,
) -> Result<(), MediaBrokerError> {
    const MAX_IMAGE_EDGE: u32 = 8_192;
    const MAX_SHARED_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
    let width = crop.width();
    let height = crop.height();
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(MediaBrokerError::InvalidIdentityRequest)?;
    if worker.process_id == 0
        || worker.process_creation_time == 0
        || worker.executable_name.is_empty()
        || worker.executable_name.len() > 260
        || worker
            .executable_name
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\'))
        || width == 0
        || height == 0
        || width > MAX_IMAGE_EDGE
        || height > MAX_IMAGE_EDGE
        || bytes > MAX_SHARED_IMAGE_BYTES
    {
        return Err(MediaBrokerError::InvalidIdentityRequest);
    }
    Ok(())
}

pub(crate) fn identity_mapping_name(lease_id: &str, lease_nonce: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(lease_id.as_bytes());
    hasher.update([0]);
    hasher.update(lease_nonce.as_bytes());
    format!(r"Local\npc.identity.{}", hex::encode(hasher.finalize()))
}

fn validate_occlusion_evidence(evidence: &BrokerOcclusionEvidence) -> Result<(), MediaBrokerError> {
    let probability = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
    if !probability(evidence.face_confidence)
        || !probability(evidence.landmark_confidence)
        || !probability(evidence.visibility_ratio)
        || evidence.measured_qpc == 0
    {
        return Err(MediaBrokerError::InvalidVisualRequest);
    }
    Ok(())
}

fn validate_residual_proposal(
    lease: &VisualSourceLease,
    residual: &BrokerResidualProposal,
) -> Result<(), MediaBrokerError> {
    validate_visual_identity(&residual.worker, &lease.track)?;
    let finite_unit = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
    if residual.worker != lease.worker
        || (residual.lease_nonce_high == 0 && residual.lease_nonce_low == 0)
        || residual.worker_handle_value == 0
        || residual.adapter_luid != lease.adapter_luid
        || residual.keyed_mutex_acquire_key == 0
        || residual.keyed_mutex_release_key == 0
        || residual.keyed_mutex_acquire_key == residual.keyed_mutex_release_key
        || residual.width == 0
        || residual.height == 0
        || residual.stride_bytes < residual.width.saturating_mul(4)
        || residual.dxgi_format != 87
        || residual.alpha_mode != 1
        || residual.expires_qpc == 0
        || !finite_unit(residual.left)
        || !finite_unit(residual.top)
        || !finite_unit(residual.right)
        || !finite_unit(residual.bottom)
        || residual.left >= residual.right
        || residual.top >= residual.bottom
        || !finite_unit(residual.confidence)
        || residual.produced_qpc < lease.source_frame_qpc
    {
        return Err(MediaBrokerError::InvalidVisualRequest);
    }
    Ok(())
}

fn decode_visual_source_lease(
    payload: VisualSourceLeasePayload,
    worker: &VisualWorkerIdentity,
    track: &VisualTrackBinding,
) -> Result<VisualSourceLease, MediaBrokerError> {
    let decoded = VisualSourceLease {
        schema_version: payload.schema_version,
        broker_process_id: payload.broker_process_id,
        broker_process_creation_time: payload.broker_process_creation_time,
        broker_executable_name: payload.broker_executable_name,
        worker: VisualWorkerIdentity {
            process_id: payload.worker_process_id,
            process_creation_time: payload.worker_process_creation_time,
            executable_name: payload.worker_executable_name,
        },
        worker_handle_value: payload.worker_handle_value,
        lease_nonce_high: payload.lease_nonce_high,
        lease_nonce_low: payload.lease_nonce_low,
        adapter_luid: payload.adapter_luid,
        keyed_mutex_acquire_key: payload.keyed_mutex_acquire_key,
        keyed_mutex_release_key: payload.keyed_mutex_release_key,
        width: payload.width,
        height: payload.height,
        stride_bytes: payload.stride_bytes,
        dxgi_format: payload.dxgi_format,
        alpha_mode: payload.alpha_mode,
        expires_qpc: payload.expires_qpc,
        qpc_frequency: payload.qpc_frequency,
        cancellation_generation: payload.cancellation_generation,
        source_device_generation: payload.source_device_generation,
        source_geometry_epoch: payload.source_geometry_epoch,
        source_frame_sequence: payload.source_frame_sequence,
        source_frame_qpc: payload.source_frame_qpc,
        track: VisualTrackBinding {
            actor_id: payload.actor_id,
            track_id: payload.track_id,
            track_epoch: payload.track_epoch,
        },
    };
    if decoded.schema_version != 1
        || decoded.broker_process_id == 0
        || decoded.broker_process_creation_time == 0
        || decoded.broker_executable_name.is_empty()
        || decoded
            .broker_executable_name
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\'))
        || &decoded.worker != worker
        || &decoded.track != track
        || decoded.worker_handle_value == 0
        || (decoded.lease_nonce_high == 0 && decoded.lease_nonce_low == 0)
        || decoded.adapter_luid == 0
        || decoded.keyed_mutex_acquire_key == 0
        || decoded.keyed_mutex_release_key == 0
        || decoded.keyed_mutex_acquire_key == decoded.keyed_mutex_release_key
        || decoded.width == 0
        || decoded.height == 0
        || decoded.stride_bytes < decoded.width.saturating_mul(4)
        || decoded.dxgi_format != 87
        || decoded.alpha_mode != 1
        || decoded.expires_qpc == 0
        || decoded.qpc_frequency == 0
        || decoded.cancellation_generation == 0
        || decoded.source_geometry_epoch == 0
        || decoded.source_frame_sequence == 0
        || decoded.source_frame_qpc == 0
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(decoded)
}

fn decode_identity_frame_lease(
    payload: IdentityFrameLeasePayload,
    worker: &IdentityWorkerIdentity,
    requested_crop: IdentityCrop,
    expected_session_id: &str,
) -> Result<IdentityFrameLease, MediaBrokerError> {
    let lease = IdentityFrameLease {
        schema_version: payload.schema_version,
        worker: IdentityWorkerIdentity {
            process_id: payload.worker_process_id,
            process_creation_time: payload.worker_process_creation_time,
            executable_name: payload.worker_executable_name,
        },
        lease_id: Zeroizing::new(payload.lease_id),
        shared_memory_name: Zeroizing::new(payload.shared_memory_name),
        lease_nonce: Zeroizing::new(payload.lease_nonce),
        byte_length: payload.byte_length,
        width: payload.width,
        height: payload.height,
        stride_bytes: payload.stride_bytes,
        pixel_format: payload.pixel_format,
        content_sha256: payload.content_sha256,
        expires_qpc: payload.expires_qpc,
        qpc_frequency: payload.qpc_frequency,
        cancellation_generation: payload.cancellation_generation,
        capture_session_id: payload.capture_session_id,
        selected_process_id: payload.selected_process_id,
        selected_window_handle: payload.selected_window_handle,
        selected_executable_name: payload.selected_executable_name,
        source_device_generation: payload.source_device_generation,
        source_geometry_epoch: payload.source_geometry_epoch,
        source_frame_sequence: payload.source_frame_sequence,
        source_frame_qpc: payload.source_frame_qpc,
        captured_at_unix_ms: payload.captured_at_unix_ms,
        advancing_frame_verified: payload.advancing_frame_verified,
        overlay_capture_excluded: payload.overlay_capture_excluded,
        protected_online_detected: payload.protected_online_detected,
        anti_cheat_detected: payload.anti_cheat_detected,
        crop: IdentityCrop {
            left: payload.crop_left,
            top: payload.crop_top,
            right: payload.crop_right,
            bottom: payload.crop_bottom,
        },
        source_width: payload.source_width,
        source_height: payload.source_height,
    };
    validate_identity_request(&lease.worker, lease.crop)?;
    let expected_mapping = identity_mapping_name(lease.lease_id(), lease.lease_nonce());
    let expected_bytes = u64::from(lease.width)
        .checked_mul(u64::from(lease.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(MediaBrokerError::Malformed)?;
    if lease.schema_version != 1
        || &lease.worker != worker
        || lease.capture_session_id != expected_session_id
        || lease.lease_id().is_empty()
        || lease.lease_id().len() > 256
        || lease.lease_nonce().is_empty()
        || lease.lease_nonce().len() > 256
        || lease.shared_memory_name() != expected_mapping
        || lease.shared_memory_name().len() > 128
        || lease.crop != requested_crop
        || lease.width != lease.crop.width()
        || lease.height != lease.crop.height()
        || lease.source_width < lease.crop.right
        || lease.source_height < lease.crop.bottom
        || lease.stride_bytes != lease.width.saturating_mul(4)
        || lease.byte_length != expected_bytes
        || lease.pixel_format != "b8g8r8a8_unorm"
        || !valid_lower_sha256(&lease.content_sha256)
        || lease.expires_qpc == 0
        || lease.qpc_frequency == 0
        || lease.cancellation_generation == 0
        || lease.selected_process_id == 0
        || lease.selected_window_handle == 0
        || lease.selected_executable_name.is_empty()
        || lease
            .selected_executable_name
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\'))
        || lease.source_device_generation == 0
        || lease.source_geometry_epoch == 0
        || lease.source_frame_sequence == 0
        || lease.source_frame_qpc == 0
        || lease.captured_at_unix_ms == 0
        || !lease.advancing_frame_verified
        || !lease.overlay_capture_excluded
        || lease.protected_online_detected
        || lease.anti_cheat_detected
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(lease)
}

fn valid_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_identity_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_identity_reference_import_request(
    request: &IdentityReferenceImportRequest,
) -> Result<(), MediaBrokerError> {
    validate_identity_request(
        &request.worker,
        IdentityCrop {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        },
    )?;
    let private_rights = request.source_class == IdentityReferenceSourceClass::UserPrivate
        && request.explicit_user_consent
        && request
            .owner_user_id
            .as_deref()
            .is_some_and(|value| valid_identity_identifier(value, 128))
        && request.original_work_license.is_none();
    let original_rights = request.source_class == IdentityReferenceSourceClass::OriginalSynthetic
        && request.owner_user_id.is_none()
        && request
            .original_work_license
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty() && value.len() <= 512);
    if !valid_identity_identifier(&request.picker_consent_token, 128)
        || !valid_identity_identifier(&request.game_profile_id, 128)
        || !valid_identity_identifier(&request.character_id, 128)
        || request.subject_id != request.character_id
        || !valid_identity_identifier(&request.reference_id, 128)
        || request.subject_display_name.trim().is_empty()
        || request.subject_display_name.len() > 256
        || !request.local_only
        || request.imported_at_unix_ms == 0
        || (!private_rights && !original_rights)
    {
        return Err(MediaBrokerError::InvalidIdentityRequest);
    }
    Ok(())
}

fn decode_identity_reference_import_lease(
    payload: IdentityReferenceImportLeasePayload,
    request: &IdentityReferenceImportRequest,
    expected_session_id: &str,
) -> Result<IdentityReferenceImportLease, MediaBrokerError> {
    let source_class = match IdentityReferenceSourceClassPayload::try_from(payload.source_class) {
        Ok(IdentityReferenceSourceClassPayload::UserPrivate) => {
            IdentityReferenceSourceClass::UserPrivate
        }
        Ok(IdentityReferenceSourceClassPayload::OriginalSynthetic) => {
            IdentityReferenceSourceClass::OriginalSynthetic
        }
        _ => return Err(MediaBrokerError::Malformed),
    };
    let lease = IdentityReferenceImportLease {
        schema_version: payload.schema_version,
        worker: IdentityWorkerIdentity {
            process_id: payload.worker_process_id,
            process_creation_time: payload.worker_process_creation_time,
            executable_name: payload.worker_executable_name,
        },
        lease_id: Zeroizing::new(payload.lease_id),
        shared_memory_name: Zeroizing::new(payload.shared_memory_name),
        lease_nonce: Zeroizing::new(payload.lease_nonce),
        byte_length: payload.byte_length,
        width: payload.width,
        height: payload.height,
        stride_bytes: payload.stride_bytes,
        pixel_format: payload.pixel_format,
        content_sha256: payload.content_sha256,
        source_asset_sha256: payload.source_asset_sha256,
        source_media_type: payload.source_media_type,
        expires_qpc: payload.expires_qpc,
        qpc_frequency: payload.qpc_frequency,
        cancellation_generation: payload.cancellation_generation,
        capture_session_id: payload.capture_session_id,
        selected_process_id: payload.selected_process_id,
        selected_window_handle: payload.selected_window_handle,
        selected_executable_name: payload.selected_executable_name,
        source_device_generation: payload.source_device_generation,
        source_geometry_epoch: payload.source_geometry_epoch,
        picker_consent_token: payload.picker_consent_token,
        game_profile_id: payload.game_profile_id,
        character_id: payload.character_id,
        subject_id: payload.subject_id,
        reference_id: payload.reference_id,
        subject_display_name: payload.subject_display_name,
        source_class,
        owner_user_id: (!payload.owner_user_id.is_empty()).then_some(payload.owner_user_id),
        original_work_license: (!payload.original_work_license.is_empty())
            .then_some(payload.original_work_license),
        explicit_user_consent: payload.explicit_user_consent,
        local_only: payload.local_only,
        imported_at_unix_ms: payload.imported_at_unix_ms,
    };
    let expected_bytes = u64::from(lease.width)
        .checked_mul(u64::from(lease.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(MediaBrokerError::Malformed)?;
    if lease.schema_version != 1
        || lease.worker != request.worker
        || lease.capture_session_id != expected_session_id
        || lease.lease_id().is_empty()
        || lease.lease_id().len() > 256
        || lease.lease_nonce().is_empty()
        || lease.lease_nonce().len() > 256
        || lease.shared_memory_name()
            != identity_mapping_name(lease.lease_id(), lease.lease_nonce())
        || lease.byte_length != expected_bytes
        || lease.width == 0
        || lease.height == 0
        || lease.width > 8_192
        || lease.height > 8_192
        || lease.byte_length > 64 * 1024 * 1024
        || lease.stride_bytes != lease.width.saturating_mul(4)
        || lease.pixel_format != "b8g8r8a8_unorm"
        || !valid_lower_sha256(&lease.content_sha256)
        || !valid_lower_sha256(&lease.source_asset_sha256)
        || !matches!(lease.source_media_type.as_str(), "image/png" | "image/jpeg")
        || lease.expires_qpc == 0
        || lease.qpc_frequency == 0
        || lease.cancellation_generation == 0
        || lease.selected_process_id == 0
        || lease.selected_window_handle == 0
        || lease.selected_executable_name.is_empty()
        || lease.selected_executable_name.len() > 260
        || lease.source_device_generation == 0
        || lease.source_geometry_epoch == 0
        || lease.picker_consent_token != request.picker_consent_token
        || lease.game_profile_id != request.game_profile_id
        || lease.character_id != request.character_id
        || lease.subject_id != request.subject_id
        || lease.reference_id != request.reference_id
        || lease.subject_display_name != request.subject_display_name
        || lease.source_class != request.source_class
        || lease.owner_user_id != request.owner_user_id
        || lease.original_work_license != request.original_work_license
        || lease.explicit_user_consent != request.explicit_user_consent
        || lease.local_only != request.local_only
        || lease.imported_at_unix_ms != request.imported_at_unix_ms
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(lease)
}

fn decode_presentation_receipt(
    response: &BrokerResponse,
    lease: &VisualSourceLease,
) -> Result<BrokerPresentationReceipt, MediaBrokerError> {
    if response.payload.len() != 8 && response.payload.len() != 12 {
        return Err(MediaBrokerError::Malformed);
    }
    let contract = read_u32(&response.payload, 0)?;
    let presented_raw = read_u32(&response.payload, 4)?;
    if presented_raw > 1 {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(BrokerPresentationReceipt {
        schema_version: 1,
        response_sequence: response.response_to_sequence,
        cancellation_generation: response.cancellation_generation,
        source_frame_sequence: lease.source_frame_sequence,
        source_frame_qpc: lease.source_frame_qpc,
        actor_id: lease.track.actor_id,
        track_id: lease.track.track_id,
        track_epoch: lease.track.track_epoch,
        residual_contract_status: contract,
        presented: response.status() == BrokerStatus::Ok && presented_raw == 1,
        failure_code: (response.payload.len() == 12)
            .then(|| read_u32(&response.payload, 8))
            .transpose()?,
    })
}

fn validate_playback_pool_request(
    request: &AudioPlaybackPoolRequest,
) -> Result<(), MediaBrokerError> {
    let valid_identifier = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && !value.contains("..")
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
    };
    if !valid_identifier(&request.session_id)
        || !valid_identifier(&request.turn_id)
        || request.generation == 0
        || !(8_000..=192_000).contains(&request.sample_rate)
        || !(1..=2).contains(&request.channels)
        || request.max_frames_per_lease == 0
        || request.max_frames_per_lease > u64::from(request.sample_rate).saturating_mul(10 * 60)
        || !(1..=MAX_PLAYBACK_LEASES_PER_TURN).contains(&request.lease_count)
    {
        return Err(MediaBrokerError::InvalidPlaybackRequest);
    }
    Ok(())
}

fn decode_playback_lease(
    payload: PlaybackLeasePayload,
    request: &AllocatePlaybackStreamPayload,
) -> Result<AudioPlaybackLease, MediaBrokerError> {
    let output_selection_mode = match payload.output_selection_mode() {
        AudioOutputSelectionModePayload::SystemDefault => AudioOutputSelectionMode::SystemDefault,
        AudioOutputSelectionModePayload::EndpointId => AudioOutputSelectionMode::EndpointId,
        AudioOutputSelectionModePayload::Unspecified => return Err(MediaBrokerError::Malformed),
    };
    let token_bytes = Zeroizing::new(payload.one_time_token);
    let token: [u8; 32] = token_bytes
        .as_slice()
        .try_into()
        .map_err(|_| MediaBrokerError::Malformed)?;
    let channels = u16::try_from(payload.channels).map_err(|_| MediaBrokerError::Malformed)?;
    let endpoint_prefix = r"\\.\pipe\npc-media-playback-";
    if payload.schema_version != 2
        || payload.stream_id.is_empty()
        || payload.stream_id.len() > 128
        || !payload.producer_endpoint.starts_with(endpoint_prefix)
        || payload.producer_endpoint.len() > 256
        || token.iter().all(|byte| *byte == 0)
        || payload.session_id != request.session_id
        || payload.turn_id != request.turn_id
        || payload.generation != request.generation
        || payload.sample_rate != request.sample_rate
        || u32::from(channels) != request.channels
        || payload.max_frames != request.max_frames
        || payload.max_chunk_bytes != 64 * 1024
        || payload.expires_qpc == 0
        || payload.output_endpoint_id.is_empty()
        || payload.output_endpoint_id.len() > 1024
        || payload.output_endpoint_generation == 0
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(AudioPlaybackLease {
        schema_version: payload.schema_version,
        stream_id: payload.stream_id,
        producer_endpoint: payload.producer_endpoint,
        one_time_token: PlaybackToken(token),
        session_id: payload.session_id,
        turn_id: payload.turn_id,
        generation: payload.generation,
        sample_rate: payload.sample_rate,
        channels,
        max_frames: payload.max_frames,
        max_chunk_bytes: payload.max_chunk_bytes,
        expires_qpc: payload.expires_qpc,
        output_selection_mode,
        output_endpoint_id: payload.output_endpoint_id,
        output_endpoint_generation: payload.output_endpoint_generation,
    })
}

fn valid_endpoint_id(endpoint_id: &str) -> bool {
    !endpoint_id.is_empty() && endpoint_id.len() <= 1024 && !endpoint_id.contains('\0')
}

fn encode_audio_output_selection(
    selection: &AudioOutputSelection,
) -> Result<SelectAudioOutputPayload, MediaBrokerError> {
    match selection {
        AudioOutputSelection::SystemDefault => Ok(SelectAudioOutputPayload {
            mode: AudioOutputSelectionModePayload::SystemDefault as i32,
            endpoint_id: String::new(),
        }),
        AudioOutputSelection::EndpointId { endpoint_id } if valid_endpoint_id(endpoint_id) => {
            Ok(SelectAudioOutputPayload {
                mode: AudioOutputSelectionModePayload::EndpointId as i32,
                endpoint_id: endpoint_id.clone(),
            })
        }
        AudioOutputSelection::EndpointId { .. } => Err(MediaBrokerError::InvalidAudioOutput),
    }
}

fn decode_audio_output_endpoint(
    payload: AudioOutputEndpointPayload,
) -> Result<AudioOutputEndpoint, MediaBrokerError> {
    let state = match payload.state {
        1 => AudioOutputState::Active,
        2 => AudioOutputState::Disabled,
        3 => AudioOutputState::NotPresent,
        4 => AudioOutputState::Unplugged,
        _ => return Err(MediaBrokerError::Malformed),
    };
    if payload.endpoint_id.is_empty()
        || payload.endpoint_id.len() > 1024
        || payload.endpoint_id.contains('\0')
        || payload.friendly_name.is_empty()
        || payload.friendly_name.len() > 512
        || payload.friendly_name.contains('\0')
        || payload.generation == 0
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(AudioOutputEndpoint {
        endpoint_id: payload.endpoint_id,
        friendly_name: payload.friendly_name,
        state,
        system_default: payload.system_default,
        generation: payload.generation,
    })
}

fn decode_audio_output_snapshot(
    payload: AudioOutputSnapshotPayload,
) -> Result<AudioOutputSnapshot, MediaBrokerError> {
    if payload.schema_version != 1
        || payload.catalog_generation == 0
        || payload.endpoints.len() > 128
    {
        return Err(MediaBrokerError::Malformed);
    }
    let endpoints = payload
        .endpoints
        .into_iter()
        .map(decode_audio_output_endpoint)
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids = std::collections::BTreeSet::new();
    if !endpoints
        .iter()
        .all(|endpoint| ids.insert(endpoint.endpoint_id.clone()))
        || endpoints
            .iter()
            .filter(|endpoint| endpoint.system_default)
            .count()
            > 1
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(AudioOutputSnapshot {
        schema_version: payload.schema_version,
        catalog_generation: payload.catalog_generation,
        endpoints,
    })
}

fn decode_selected_audio_output(
    payload: SelectedAudioOutputPayload,
) -> Result<SelectedAudioOutput, MediaBrokerError> {
    if payload.schema_version != 1 {
        return Err(MediaBrokerError::Malformed);
    }
    let mode = payload.mode();
    let resolved =
        decode_audio_output_endpoint(payload.resolved.ok_or(MediaBrokerError::Malformed)?)?;
    let selection = match mode {
        AudioOutputSelectionModePayload::SystemDefault
            if payload.requested_endpoint_id.is_empty() && resolved.system_default =>
        {
            AudioOutputSelection::SystemDefault
        }
        AudioOutputSelectionModePayload::EndpointId
            if payload.requested_endpoint_id == resolved.endpoint_id =>
        {
            AudioOutputSelection::EndpointId {
                endpoint_id: payload.requested_endpoint_id,
            }
        }
        _ => return Err(MediaBrokerError::Malformed),
    };
    if resolved.state != AudioOutputState::Active {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(SelectedAudioOutput {
        schema_version: payload.schema_version,
        selection,
        resolved,
    })
}

fn encode_audio_input_selection(
    selection: &AudioInputSelection,
) -> Result<SelectAudioOutputPayload, MediaBrokerError> {
    match selection {
        AudioInputSelection::SystemDefault => Ok(SelectAudioOutputPayload {
            mode: AudioOutputSelectionModePayload::SystemDefault as i32,
            endpoint_id: String::new(),
        }),
        AudioInputSelection::EndpointId { endpoint_id } if valid_endpoint_id(endpoint_id) => {
            Ok(SelectAudioOutputPayload {
                mode: AudioOutputSelectionModePayload::EndpointId as i32,
                endpoint_id: endpoint_id.clone(),
            })
        }
        AudioInputSelection::EndpointId { .. } => Err(MediaBrokerError::InvalidAudioInput),
    }
}

fn decode_audio_input_endpoint(
    payload: AudioInputEndpointPayload,
) -> Result<AudioInputEndpoint, MediaBrokerError> {
    let state = match payload.state {
        1 => AudioInputState::Active,
        2 => AudioInputState::Disabled,
        3 => AudioInputState::NotPresent,
        4 => AudioInputState::Unplugged,
        _ => return Err(MediaBrokerError::Malformed),
    };
    if !valid_endpoint_id(&payload.endpoint_id)
        || payload.friendly_name.is_empty()
        || payload.friendly_name.len() > 512
        || payload.friendly_name.contains('\0')
        || payload.generation == 0
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(AudioInputEndpoint {
        endpoint_id: payload.endpoint_id,
        friendly_name: payload.friendly_name,
        state,
        system_default: payload.system_default,
        generation: payload.generation,
    })
}

fn decode_audio_input_snapshot(
    payload: AudioInputSnapshotPayload,
) -> Result<AudioInputSnapshot, MediaBrokerError> {
    if payload.schema_version != 1
        || payload.catalog_generation == 0
        || payload.endpoints.len() > 128
    {
        return Err(MediaBrokerError::Malformed);
    }
    let endpoints = payload
        .endpoints
        .into_iter()
        .map(decode_audio_input_endpoint)
        .collect::<Result<Vec<_>, _>>()?;
    let default_count = endpoints
        .iter()
        .filter(|endpoint| endpoint.system_default)
        .count();
    if default_count > 1
        || endpoints.iter().enumerate().any(|(index, endpoint)| {
            endpoints[index + 1..]
                .iter()
                .any(|other| other.endpoint_id == endpoint.endpoint_id)
        })
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(AudioInputSnapshot {
        schema_version: payload.schema_version,
        catalog_generation: payload.catalog_generation,
        endpoints,
    })
}

fn decode_selected_audio_input(
    payload: SelectedAudioInputPayload,
) -> Result<SelectedAudioInput, MediaBrokerError> {
    if payload.schema_version != 1 {
        return Err(MediaBrokerError::Malformed);
    }
    let resolved =
        decode_audio_input_endpoint(payload.resolved.ok_or(MediaBrokerError::Malformed)?)?;
    let selection = match AudioOutputSelectionModePayload::try_from(payload.mode) {
        Ok(AudioOutputSelectionModePayload::SystemDefault)
            if payload.requested_endpoint_id.is_empty() && resolved.system_default =>
        {
            AudioInputSelection::SystemDefault
        }
        Ok(AudioOutputSelectionModePayload::EndpointId)
            if payload.requested_endpoint_id == resolved.endpoint_id =>
        {
            AudioInputSelection::EndpointId {
                endpoint_id: payload.requested_endpoint_id,
            }
        }
        _ => return Err(MediaBrokerError::Malformed),
    };
    if resolved.state != AudioInputState::Active {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(SelectedAudioInput {
        schema_version: payload.schema_version,
        selection,
        resolved,
    })
}

fn decode_ptt_activation_state(
    payload: PttActivationStatePayload,
) -> Result<PttActivationState, MediaBrokerError> {
    let state = match payload.state {
        0 => PttActivationStateKind::Released,
        1 => PttActivationStateKind::Pressed,
        _ => return Err(MediaBrokerError::Malformed),
    };
    let coherent = match state {
        PttActivationStateKind::Released => {
            payload.release_transition_sequence == payload.transition_sequence
                && payload.released_qpc == payload.transition_qpc
        }
        PttActivationStateKind::Pressed => {
            payload.release_transition_sequence < payload.transition_sequence
                && payload.released_qpc < payload.transition_qpc
        }
    };
    if payload.schema_version != 1
        || payload.virtual_key == 0
        || payload.transition_sequence == 0
        || payload.transition_qpc == 0
        || payload.release_transition_sequence == 0
        || payload.release_transition_sequence > payload.transition_sequence
        || payload.released_qpc == 0
        || payload.released_qpc > payload.transition_qpc
        || !coherent
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(PttActivationState {
        schema_version: payload.schema_version,
        virtual_key: payload.virtual_key,
        state,
        transition_sequence: payload.transition_sequence,
        transition_qpc: payload.transition_qpc,
        release_transition_sequence: payload.release_transition_sequence,
        released_qpc: payload.released_qpc,
    })
}

fn validate_audio_input_rehearsal_request(
    request: &AudioInputRehearsalRequest,
) -> Result<(), MediaBrokerError> {
    validate_bounded_identifier(&request.session_id, 128)?;
    validate_bounded_identifier(&request.turn_id, 128)?;
    let duration_frame_limit = u64::from(request.sample_rate)
        .checked_mul(u64::from(request.duration_ms))
        .and_then(|value| value.checked_div(1000))
        .ok_or(MediaBrokerError::InvalidAudioInputRequest)?;
    if request.generation == 0
        || !(MIN_INPUT_REHEARSAL_DURATION_MS..=MAX_INPUT_REHEARSAL_DURATION_MS)
            .contains(&request.duration_ms)
        || !(8_000..=192_000).contains(&request.sample_rate)
        || request.channels == 0
        || request.channels > 2
        || request.max_frames == 0
        || request.max_frames > duration_frame_limit
        || request.activation_source != InputActivationSource::PushToTalk
    {
        return Err(MediaBrokerError::InvalidAudioInputRequest);
    }
    Ok(())
}

fn validate_bounded_identifier(value: &str, maximum: usize) -> Result<(), MediaBrokerError> {
    if value.is_empty() || value.len() > maximum || value.contains('\0') {
        return Err(MediaBrokerError::InvalidAudioInputRequest);
    }
    Ok(())
}

fn decode_audio_input_rehearsal_lease(
    payload: AudioInputRehearsalLeasePayload,
    request: &AllocateAudioInputRehearsalPayload,
) -> Result<AudioInputRehearsalLease, MediaBrokerError> {
    let input_selection_mode =
        match AudioOutputSelectionModePayload::try_from(payload.input_selection_mode) {
            Ok(AudioOutputSelectionModePayload::SystemDefault) => {
                AudioOutputSelectionMode::SystemDefault
            }
            Ok(AudioOutputSelectionModePayload::EndpointId) => AudioOutputSelectionMode::EndpointId,
            _ => return Err(MediaBrokerError::Malformed),
        };
    let activation_source = match InputActivationSourcePayload::try_from(payload.activation_source)
    {
        Ok(InputActivationSourcePayload::PushToTalk) => InputActivationSource::PushToTalk,
        Ok(InputActivationSourcePayload::ExplicitRehearsal) => {
            InputActivationSource::ExplicitRehearsal
        }
        _ => return Err(MediaBrokerError::Malformed),
    };
    let token_bytes = Zeroizing::new(payload.one_time_token);
    let token: [u8; 32] = token_bytes
        .as_slice()
        .try_into()
        .map_err(|_| MediaBrokerError::Malformed)?;
    if payload.schema_version != 2
        || payload.session_id != request.session_id
        || payload.turn_id != request.turn_id
        || payload.generation != request.generation
        || payload.duration_ms != request.duration_ms
        || payload.sample_rate != request.sample_rate
        || payload.channels != request.channels
        || payload.max_frames != request.max_frames
        || payload.activation_source != request.activation_source
        || payload.max_chunk_bytes != 64 * 1024
        || payload.expires_qpc == 0
        || payload.qpc_frequency == 0
        || !valid_endpoint_id(&payload.input_endpoint_id)
        || payload.input_endpoint_generation == 0
        || (activation_source == InputActivationSource::PushToTalk
            && (payload.ptt_virtual_key == 0
                || payload.ptt_virtual_key > 0xff
                || payload.ptt_press_transition_sequence == 0
                || payload.ptt_pressed_qpc == 0))
        || (activation_source == InputActivationSource::ExplicitRehearsal
            && (payload.ptt_virtual_key != 0
                || payload.ptt_press_transition_sequence != 0
                || payload.ptt_pressed_qpc != 0))
        || payload.stream_id.is_empty()
        || payload.stream_id.len() > 128
        || payload.stream_id.contains('\0')
        || !payload
            .producer_endpoint
            .starts_with(r"\\.\pipe\npc-media-input-")
        || payload.producer_endpoint.len() > 256
        || payload.producer_endpoint.contains('\0')
    {
        return Err(MediaBrokerError::Malformed);
    }
    let channels = u16::try_from(payload.channels).map_err(|_| MediaBrokerError::Malformed)?;
    Ok(AudioInputRehearsalLease {
        schema_version: payload.schema_version,
        stream_id: payload.stream_id,
        producer_endpoint: payload.producer_endpoint,
        one_time_token: InputLeaseToken(token),
        session_id: payload.session_id,
        turn_id: payload.turn_id,
        generation: payload.generation,
        duration_ms: payload.duration_ms,
        sample_rate: payload.sample_rate,
        channels,
        max_frames: payload.max_frames,
        max_chunk_bytes: payload.max_chunk_bytes,
        expires_qpc: payload.expires_qpc,
        qpc_frequency: payload.qpc_frequency,
        input_selection_mode,
        input_endpoint_id: payload.input_endpoint_id,
        input_endpoint_generation: payload.input_endpoint_generation,
        activation_source,
        ptt_virtual_key: payload.ptt_virtual_key,
        ptt_press_transition_sequence: payload.ptt_press_transition_sequence,
        ptt_pressed_qpc: payload.ptt_pressed_qpc,
    })
}

fn valid_presentation_rect(rect: PresentationPixelRect) -> bool {
    rect.right > rect.left && rect.bottom > rect.top
}

fn validate_visual_audio_envelope_query(
    query: &VisualAudioEnvelopeQuery,
) -> Result<(), MediaBrokerError> {
    let valid = |value: &str| !value.is_empty() && value.len() <= 128 && !value.contains('\0');
    if !valid(&query.session_id)
        || !valid(&query.turn_id)
        || query.generation == 0
        || !valid(&query.stream_id)
        || !valid(&query.segment_id)
        || query.segment_id != query.stream_id
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(())
}

fn decode_visual_audio_envelope(
    payload: VisualAudioEnvelopePayload,
    query: &VisualAudioEnvelopeQuery,
) -> Result<VisualAudioEnvelope, MediaBrokerError> {
    validate_visual_audio_envelope_query(query)?;
    let channels = u16::try_from(payload.channels).map_err(|_| MediaBrokerError::Malformed)?;
    let rms: Vec<u16> = payload
        .mono_rms_q15
        .into_iter()
        .map(|value| u16::try_from(value).map_err(|_| MediaBrokerError::Malformed))
        .collect::<Result<_, _>>()?;
    let peak: Vec<u16> = payload
        .mono_peak_q15
        .into_iter()
        .map(|value| u16::try_from(value).map_err(|_| MediaBrokerError::Malformed))
        .collect::<Result<_, _>>()?;
    let mono_rms_q15: [u16; 8] = rms.try_into().map_err(|_| MediaBrokerError::Malformed)?;
    let mono_peak_q15: [u16; 8] = peak.try_into().map_err(|_| MediaBrokerError::Malformed)?;
    if payload.visual_speech_cues.len() > MAX_VISUAL_SPEECH_CUES {
        return Err(MediaBrokerError::Malformed);
    }
    let mut visual_speech_cues = Vec::with_capacity(payload.visual_speech_cues.len());
    let mut previous_end = None;
    for cue in payload.visual_speech_cues {
        let end_sample = cue
            .start_sample
            .checked_add(cue.duration_samples)
            .ok_or(MediaBrokerError::Malformed)?;
        let canonical_viseme =
            u8::try_from(cue.canonical_viseme).map_err(|_| MediaBrokerError::Malformed)?;
        let strength_q15 =
            u16::try_from(cue.strength_q15).map_err(|_| MediaBrokerError::Malformed)?;
        if cue.duration_samples == 0
            || canonical_viseme > 10
            || strength_q15 > 32_767
            || previous_end.is_some_and(|end| cue.start_sample < end)
        {
            return Err(MediaBrokerError::Malformed);
        }
        visual_speech_cues.push(VisualSpeechCue {
            start_sample: cue.start_sample,
            duration_samples: cue.duration_samples,
            canonical_viseme,
            strength_q15,
        });
        previous_end = Some(end_sample);
    }
    let envelope = VisualAudioEnvelope {
        schema_version: payload.schema_version,
        session_id: payload.session_id,
        turn_id: payload.turn_id,
        generation: payload.generation,
        stream_id: payload.stream_id,
        segment_id: payload.segment_id,
        source_sample_start: payload.source_sample_start,
        source_sample_count: payload.source_sample_count,
        sample_rate: payload.sample_rate,
        channels,
        device_write_qpc: payload.device_write_qpc,
        qpc_frequency: payload.qpc_frequency,
        source_frames: payload.source_frames,
        device_frames: payload.device_frames,
        mono_rms_q15,
        mono_peak_q15,
        active: payload.active,
        draining: payload.draining,
        cancelled: payload.cancelled,
        visual_speech_cues,
    };
    let sample_end = envelope
        .source_sample_start
        .checked_add(u64::from(envelope.source_sample_count))
        .ok_or(MediaBrokerError::Malformed)?;
    if envelope.schema_version != 1
        || envelope.session_id != query.session_id
        || envelope.turn_id != query.turn_id
        || envelope.generation != query.generation
        || envelope.stream_id != query.stream_id
        || envelope.segment_id != query.segment_id
        || envelope.source_sample_count == 0
        || !(8_000..=192_000).contains(&envelope.sample_rate)
        || !(1..=2).contains(&envelope.channels)
        || envelope.device_write_qpc == 0
        || envelope.qpc_frequency == 0
        || envelope.source_frames < sample_end
        || envelope.device_frames == 0
        || envelope.cancelled
        || envelope.active == envelope.draining
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(envelope)
}

fn decode_trusted_subtitle_presentation_context(
    payload: TrustedSubtitlePresentationContextPayload,
) -> Result<TrustedSubtitlePresentationContext, MediaBrokerError> {
    let window_bounds_px = PresentationPixelRect {
        left: payload.window_left_px,
        top: payload.window_top_px,
        right: payload.window_right_px,
        bottom: payload.window_bottom_px,
    };
    let client_bounds_px = PresentationPixelRect {
        left: payload.client_left_px,
        top: payload.client_top_px,
        right: payload.client_right_px,
        bottom: payload.client_bottom_px,
    };
    let monitor_bounds_px = PresentationPixelRect {
        left: payload.monitor_left_px,
        top: payload.monitor_top_px,
        right: payload.monitor_right_px,
        bottom: payload.monitor_bottom_px,
    };
    let monitor_work_area_px = PresentationPixelRect {
        left: payload.work_left_px,
        top: payload.work_top_px,
        right: payload.work_right_px,
        bottom: payload.work_bottom_px,
    };
    let capture_backend = match payload.capture_backend {
        1 => TrustedCaptureBackend::WindowsGraphicsCapture,
        2 => TrustedCaptureBackend::DesktopDuplication,
        _ => return Err(MediaBrokerError::Malformed),
    };
    let capture_scope = match (payload.capture_backend, payload.capture_scope) {
        (1, 1) => TrustedCaptureScope::ExactGameHwndWgc,
        (2, 2) => TrustedCaptureScope::MonitorRegionCrop,
        _ => return Err(MediaBrokerError::Malformed),
    };
    let target_color_space = match (
        payload.target_color_space_available,
        payload.target_color_space,
    ) {
        (true, 0) => Some(TrustedTargetColorSpace::SdrSrgb),
        (true, 1) => Some(TrustedTargetColorSpace::SdrScRgb),
        (true, 2) => Some(TrustedTargetColorSpace::Hdr10Pq),
        (true, 3) => Some(TrustedTargetColorSpace::HdrScRgb),
        (false, 4) => None,
        _ => return Err(MediaBrokerError::Malformed),
    };
    let executable = payload.selected_executable_name.to_ascii_lowercase();
    let attestation_delta = payload
        .attested_at_qpc
        .checked_sub(payload.source_frame_qpc)
        .ok_or(MediaBrokerError::Malformed)?;
    if payload.schema_version != 1
        || payload.selected_process_id == 0
        || payload.selected_window == 0
        || payload.selected_executable_name.len() < 5
        || payload.selected_executable_name.len() > 260
        || payload.selected_executable_name.contains(['\0', '/', '\\'])
        || !executable.ends_with(".exe")
        || payload.capture_device_generation == 0
        || payload.geometry_epoch == 0
        || payload.source_frame_sequence == 0
        || payload.source_frame_qpc == 0
        || !valid_presentation_rect(window_bounds_px)
        || !valid_presentation_rect(client_bounds_px)
        || !valid_presentation_rect(monitor_bounds_px)
        || !valid_presentation_rect(monitor_work_area_px)
        || payload.captured_width_px == 0
        || payload.captured_height_px == 0
        || payload.monitor_id.is_empty()
        || payload.monitor_id.len() > 128
        || payload.monitor_id.contains('\0')
        || (payload.dpi_available
            && (!(48..=960).contains(&payload.dpi_x) || !(48..=960).contains(&payload.dpi_y)))
        || (!payload.dpi_available && (payload.dpi_x != 0 || payload.dpi_y != 0))
        || (!payload.hdr_evidence_available
            && (payload.hdr_supported || payload.hdr_user_enabled || payload.hdr_active))
        || (payload.hdr_active && !payload.hdr_supported)
        || (payload.color_encoding_available && payload.bits_per_color_channel == 0)
        || (!payload.color_encoding_available
            && (payload.color_encoding != 0 || payload.bits_per_color_channel != 0))
        || (payload.sdr_white_level_available
            && (!payload.sdr_white_level_nits.is_finite()
                || !(40.0..=1000.0).contains(&payload.sdr_white_level_nits)))
        || (!payload.sdr_white_level_available && payload.sdr_white_level_nits != 0.0)
        || payload.attested_at_qpc == 0
        || payload.qpc_frequency == 0
        || payload.attestation_id == 0
        || attestation_delta > payload.qpc_frequency.saturating_mul(2)
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(TrustedSubtitlePresentationContext {
        schema_version: payload.schema_version,
        selected_process_id: payload.selected_process_id,
        selected_window: payload.selected_window,
        selected_executable_name: payload.selected_executable_name,
        capture_device_generation: payload.capture_device_generation,
        geometry_epoch: payload.geometry_epoch,
        source_frame_sequence: payload.source_frame_sequence,
        source_frame_qpc: payload.source_frame_qpc,
        window_bounds_px,
        client_bounds_px,
        captured_width_px: payload.captured_width_px,
        captured_height_px: payload.captured_height_px,
        monitor_id: payload.monitor_id,
        monitor_bounds_px,
        monitor_work_area_px,
        dpi_available: payload.dpi_available,
        dpi_x: payload.dpi_x,
        dpi_y: payload.dpi_y,
        hdr_evidence_available: payload.hdr_evidence_available,
        hdr_supported: payload.hdr_supported,
        hdr_user_enabled: payload.hdr_user_enabled,
        hdr_active: payload.hdr_active,
        advanced_color_active: payload.advanced_color_active,
        active_color_mode: payload.active_color_mode,
        color_encoding_available: payload.color_encoding_available,
        color_encoding: payload.color_encoding,
        bits_per_color_channel: payload.bits_per_color_channel,
        sdr_white_level_available: payload.sdr_white_level_available,
        sdr_white_level_nits: payload.sdr_white_level_nits,
        capture_backend,
        capture_scope,
        overlay_capture_excluded: payload.overlay_capture_excluded,
        overlay_visuals_allowed: payload.overlay_visuals_allowed,
        target_color_space_available: payload.target_color_space_available,
        target_color_space,
        attested_at_qpc: payload.attested_at_qpc,
        qpc_frequency: payload.qpc_frequency,
        attestation_id: payload.attestation_id,
    })
}

fn decode_diagnostics(payload: &[u8]) -> Result<MediaBrokerDiagnostics, MediaBrokerError> {
    // v1 originally exposed ten fixed fields (80 bytes). Native residual
    // rejection counters were appended in a compatible broker update. Accept
    // both payload sizes and ignore unknown trailing diagnostics until the
    // desktop domain exposes them, rather than treating a healthy broker as an
    // authentication failure during its health refresh.
    if payload.len() != 80 && payload.len() != 104 {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(MediaBrokerDiagnostics {
        state: broker_state_name(read_u32(payload, 0)?).into(),
        capture_backend: enum_name(
            read_u32(payload, 4)?,
            &["none", "windowsGraphicsCapture", "desktopDuplication"],
        ),
        overlay_backend: enum_name(read_u32(payload, 8)?, &["none", "d3d11DirectComposition"]),
        capture_audio: enum_name(
            read_u32(payload, 12)?,
            &[
                "stopped",
                "initializing",
                "ready",
                "capturing",
                "playing",
                "recovering",
                "failed",
            ],
        ),
        render_audio: enum_name(
            read_u32(payload, 16)?,
            &[
                "stopped",
                "initializing",
                "ready",
                "capturing",
                "playing",
                "recovering",
                "failed",
            ],
        ),
        target_state: enum_name(
            read_u32(payload, 20)?,
            &[
                "none",
                "selected",
                "unavailable",
                "minimized",
                "occluded",
                "protectedContent",
                "closed",
            ],
        ),
        device_generation: read_u64(payload, 24)?,
        audio_device_generation: read_u64(payload, 32)?,
        cancellation_generation: read_u64(payload, 40)?,
        frames_received: read_u64(payload, 48)?,
        frames_presented: read_u64(payload, 56)?,
        frames_dropped: read_u64(payload, 64)?,
        overlays_suppressed: read_u64(payload, 72)?,
    })
}

fn decode_native_capture_evidence(
    payload: &[u8],
) -> Result<NativeCaptureEvidence, MediaBrokerError> {
    const FIXED_BYTES: usize = 128;
    if payload.len() < FIXED_BYTES || read_u32(payload, 0)? != 3 {
        return Err(MediaBrokerError::Malformed);
    }
    let name_bytes =
        usize::try_from(read_u32(payload, 104)?).map_err(|_| MediaBrokerError::Malformed)?;
    if name_bytes == 0
        || name_bytes > 260
        || payload.len() != FIXED_BYTES.saturating_add(name_bytes)
    {
        return Err(MediaBrokerError::Malformed);
    }
    let pixel_source = match read_u32(payload, 108)? {
        1 => CapturePixelSource::WindowsGraphicsCaptureTexture,
        2 => CapturePixelSource::DesktopDuplicationTexture,
        _ => return Err(MediaBrokerError::Malformed),
    };
    let pixel_scope = match read_u32(payload, 112)? {
        1 => CapturePixelScope::ExactSelectedWindow,
        2 => CapturePixelScope::FullDisplayOutput,
        _ => return Err(MediaBrokerError::Malformed),
    };
    let selected_executable_name = std::str::from_utf8(&payload[FIXED_BYTES..])
        .map_err(|_| MediaBrokerError::Malformed)?
        .to_owned();
    if selected_executable_name.contains(['/', '\\'])
        || !selected_executable_name
            .to_ascii_lowercase()
            .ends_with(".exe")
    {
        return Err(MediaBrokerError::Malformed);
    }
    let boolean = |offset| -> Result<bool, MediaBrokerError> {
        match read_u32(payload, offset)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(MediaBrokerError::Malformed),
        }
    };
    let evidence = NativeCaptureEvidence {
        schema_version: 3,
        selected_process_id: read_u32(payload, 4)?,
        selected_window_handle: read_u64(payload, 8)?,
        device_generation: read_u64(payload, 16)?,
        geometry_epoch: read_u64(payload, 24)?,
        latest_frame_sequence: read_u64(payload, 32)?,
        latest_frame_qpc: read_u64(payload, 40)?,
        initial_content_hash: read_u64(payload, 48)?,
        latest_content_hash: read_u64(payload, 56)?,
        content_hash_changes: read_u64(payload, 64)?,
        geometry_changes: read_u64(payload, 72)?,
        nonadvancing_frames: read_u64(payload, 80)?,
        content_width: read_u32(payload, 88)?,
        content_height: read_u32(payload, 92)?,
        overlay_capture_excluded: boolean(96)?,
        overlay_visuals_allowed: boolean(100)?,
        pixel_source,
        pixel_scope,
        external_display_overlay_pixels_excluded: boolean(116)?,
        desktop_luminance_excluded_from_pixel_evidence: boolean(120)?,
        external_display_overlays_may_change_perceived_brightness: boolean(124)?,
        selected_executable_name,
    };
    if evidence.selected_process_id == 0
        || evidence.selected_window_handle == 0
        || evidence.geometry_epoch == 0
        || evidence.latest_frame_sequence == 0
        || evidence.latest_frame_qpc == 0
        || evidence.initial_content_hash == 0
        || evidence.latest_content_hash == 0
        || evidence.content_width == 0
        || evidence.content_height == 0
    {
        return Err(MediaBrokerError::Malformed);
    }
    Ok(evidence)
}

fn broker_state_name(value: u32) -> &'static str {
    const VALUES: &[&str] = &[
        "stopped",
        "starting",
        "awaitingTarget",
        "capturingPrimary",
        "capturingFallback",
        "recoveringDevice",
        "blockedByPolicy",
        "degradedAudioOnly",
        "stopping",
        "failed",
    ];
    VALUES.get(value as usize).copied().unwrap_or("unknown")
}

fn enum_name(value: u32, values: &[&str]) -> String {
    values
        .get(value as usize)
        .copied()
        .unwrap_or("unknown")
        .into()
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, MediaBrokerError> {
    let value: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or(MediaBrokerError::Malformed)?
        .try_into()
        .map_err(|_| MediaBrokerError::Malformed)?;
    Ok(u32::from_le_bytes(value))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, MediaBrokerError> {
    let value: [u8; 8] = bytes
        .get(offset..offset + 8)
        .ok_or(MediaBrokerError::Malformed)?
        .try_into()
        .map_err(|_| MediaBrokerError::Malformed)?;
    Ok(u64::from_le_bytes(value))
}

fn qpc_now() -> (u64, u64) {
    #[cfg(windows)]
    {
        let mut value = 0_i64;
        let mut frequency = 0_i64;
        // SAFETY: both APIs write to live stack-owned outputs.
        unsafe {
            windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut value);
            windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency);
        }
        (value.max(0) as u64, frequency.max(1) as u64)
    }
    #[cfg(not(windows))]
    {
        (0, 1)
    }
}

fn unavailable_health(detail: &str) -> MediaBrokerHealthSnapshot {
    MediaBrokerHealthSnapshot {
        state: RuntimeConnectionState::Unavailable,
        connected: false,
        process_id: None,
        restart_count: 0,
        recent_failure_count: 0,
        protocol_version: None,
        fixture_only: false,
        broker_state: None,
        capture_available: false,
        overlay_available: false,
        capture_audio_available: false,
        render_audio_available: false,
        detail: detail.into(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MediaBrokerError {
    #[error("bundled media broker is missing")]
    Missing,
    #[error("bundled media broker layout is invalid")]
    InvalidBundle,
    #[error("media broker launch nonce generation failed")]
    Random,
    #[error("media broker process could not be started or inspected")]
    Process,
    #[error("media broker could not join the parent Job Object: {0}")]
    Job(String),
    #[error("media broker control endpoint is unavailable")]
    Connection,
    #[error("media broker control endpoint is unavailable (Windows code {0})")]
    ConnectionCode(u32),
    #[error("media broker control request timed out")]
    Timeout,
    #[error("media broker control frame exceeded its bound")]
    Payload,
    #[error("media broker control response was malformed")]
    Malformed,
    #[error("media broker rejected the request with status {0:?}")]
    Remote(BrokerStatus),
    #[error("media broker supervisor is quarantined")]
    Quarantined,
    #[error("media broker is skipped in this unbundled development build")]
    DevelopmentFixture,
    #[error("media broker supervisor state is unavailable")]
    State,
    #[error("supervised runtime process is unavailable for playback binding")]
    RuntimeUnavailable,
    #[error("supervised runtime could not be prepared for playback: {0}")]
    Runtime(String),
    #[error("playback pool request is invalid or exceeds its bound")]
    InvalidPlaybackRequest,
    #[error("audio output selection is required before production playback")]
    AudioOutputSelectionRequired,
    #[error("audio output selection is invalid")]
    InvalidAudioOutput,
    #[error("audio output selection could not be persisted: {0}")]
    AudioOutputPersistence(String),
    #[error("audio input selection is required before microphone capture")]
    AudioInputSelectionRequired,
    #[error("audio input selection is invalid")]
    InvalidAudioInput,
    #[error("audio input selection could not be persisted: {0}")]
    AudioInputPersistence(String),
    #[error("audio input rehearsal request violates the bounded native contract")]
    InvalidAudioInputRequest,
    #[error("visual source or residual request violates the typed product contract")]
    InvalidVisualRequest,
    #[error("manual actor picker request violates the private native contract")]
    InvalidManualActorPickerRequest,
    #[error("identity frame request violates the bounded CPU observation contract")]
    InvalidIdentityRequest,
    #[cfg(debug_assertions)]
    #[error("synthetic replay capture target is not the dedicated debug executable")]
    DebugSyntheticTargetInvalid,
    #[cfg(debug_assertions)]
    #[error("synthetic replay capture target does not match the selected debug process")]
    DebugSyntheticTargetMismatch,
    #[cfg(debug_assertions)]
    #[error("synthetic replay capture metadata is unavailable in the fixed app-data handoff")]
    DebugSyntheticMetadataUnavailable,
    #[cfg(debug_assertions)]
    #[error("synthetic replay capture metadata is malformed or not ready")]
    DebugSyntheticMetadataInvalid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protobuf_codec_matches_health_envelope_contract() {
        let envelope = BrokerEnvelope {
            version: 1,
            launch_nonce: vec![7; 32],
            session_id: "media-fixture-session".into(),
            sequence: 1,
            deadline_qpc: 42,
            cancellation_generation: 0,
            command: BrokerCommand::Health as i32,
            payload: Vec::new(),
        };
        let decoded = BrokerEnvelope::decode(envelope.encode_to_vec().as_slice()).expect("decode");
        assert_eq!(decoded, envelope);
        assert_eq!(decoded.launch_nonce.len(), 32);
    }

    #[test]
    fn identity_frame_lease_preserves_exact_attested_crop_and_hides_capabilities() {
        assert_eq!(BrokerCommand::AllocateIdentityFrame as i32, 16);
        assert_eq!(BrokerCommand::ReleaseIdentityFrame as i32, 17);
        let worker = IdentityWorkerIdentity {
            process_id: 445,
            process_creation_time: 0x2233_4455,
            executable_name: "python.exe".into(),
        };
        let crop = IdentityCrop {
            left: 32,
            top: 48,
            right: 288,
            bottom: 304,
        };
        let lease_id = "00112233445566778899aabbccddeeff";
        let lease_nonce = "ffeeddccbbaa99887766554433221100";
        let payload = IdentityFrameLeasePayload {
            schema_version: 1,
            worker_process_id: worker.process_id,
            worker_process_creation_time: worker.process_creation_time,
            worker_executable_name: worker.executable_name.clone(),
            lease_id: lease_id.into(),
            shared_memory_name: identity_mapping_name(lease_id, lease_nonce),
            lease_nonce: lease_nonce.into(),
            byte_length: 256 * 256 * 4,
            width: 256,
            height: 256,
            stride_bytes: 1024,
            pixel_format: "b8g8r8a8_unorm".into(),
            content_sha256: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                .into(),
            expires_qpc: 9_000,
            qpc_frequency: 10_000_000,
            cancellation_generation: 3,
            capture_session_id: "identity-session".into(),
            selected_process_id: 123,
            selected_window_handle: 0x9988,
            selected_executable_name: "game.exe".into(),
            source_device_generation: 9,
            source_geometry_epoch: 7,
            source_frame_sequence: 12,
            source_frame_qpc: 8_400,
            captured_at_unix_ms: 1_700_000_000_000,
            advancing_frame_verified: true,
            overlay_capture_excluded: true,
            protected_online_detected: false,
            anti_cheat_detected: false,
            crop_left: crop.left,
            crop_top: crop.top,
            crop_right: crop.right,
            crop_bottom: crop.bottom,
            source_width: 1280,
            source_height: 720,
        };
        let lease = decode_identity_frame_lease(payload.clone(), &worker, crop, "identity-session")
            .expect("exact identity lease");
        assert_eq!(lease.crop, crop);
        assert_eq!(lease.source_frame_sequence, 12);
        assert_eq!(lease.source_frame_qpc, 8_400);
        assert_eq!(lease.selected_window_handle, 0x9988);
        assert_eq!(
            lease.shared_memory_name(),
            identity_mapping_name(lease_id, lease_nonce)
        );
        let debug = format!("{lease:?}");
        assert!(!debug.contains(lease_id));
        assert!(!debug.contains(lease_nonce));
        assert!(!debug.contains("Local\\npc.identity."));

        let mut wrong_mapping = payload.clone();
        wrong_mapping.shared_memory_name =
            "Local\\npc.identity.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .into();
        assert!(
            decode_identity_frame_lease(wrong_mapping, &worker, crop, "identity-session").is_err()
        );
        let mut unsafe_evidence = payload;
        unsafe_evidence.overlay_capture_excluded = false;
        assert!(
            decode_identity_frame_lease(unsafe_evidence, &worker, crop, "identity-session")
                .is_err()
        );
    }

    #[test]
    fn identity_frame_decoder_rejects_unbound_mapping_unsafe_evidence_and_oversize_crop() {
        let worker = IdentityWorkerIdentity {
            process_id: 445,
            process_creation_time: 0x2233_4455,
            executable_name: "python.exe".into(),
        };
        assert!(validate_identity_request(
            &worker,
            IdentityCrop {
                left: 0,
                top: 0,
                right: 8192,
                bottom: 8192
            }
        )
        .is_err());
        assert_eq!(
            identity_mapping_name("lease-a", "nonce-b"),
            format!(
                "Local\\npc.identity.{}",
                hex::encode(Sha256::digest(b"lease-a\0nonce-b"))
            )
        );
    }

    fn private_reference_request() -> IdentityReferenceImportRequest {
        IdentityReferenceImportRequest {
            worker: IdentityWorkerIdentity {
                process_id: 445,
                process_creation_time: 0x2233_4455,
                executable_name: "python.exe".into(),
            },
            picker_consent_token: "consent-001".into(),
            game_profile_id: "game-001".into(),
            character_id: "mara".into(),
            subject_id: "mara".into(),
            reference_id: "reference-001".into(),
            subject_display_name: "Mara".into(),
            source_class: IdentityReferenceSourceClass::UserPrivate,
            owner_user_id: Some("local-user".into()),
            original_work_license: None,
            explicit_user_consent: true,
            local_only: true,
            imported_at_unix_ms: 1_700_000_000_000,
        }
    }

    fn reference_lease_payload(
        request: &IdentityReferenceImportRequest,
    ) -> IdentityReferenceImportLeasePayload {
        let lease_id = "00112233445566778899aabbccddeeff";
        let lease_nonce = "ffeeddccbbaa99887766554433221100";
        IdentityReferenceImportLeasePayload {
            schema_version: 1,
            worker_process_id: request.worker.process_id,
            worker_process_creation_time: request.worker.process_creation_time,
            worker_executable_name: request.worker.executable_name.clone(),
            lease_id: lease_id.into(),
            shared_memory_name: identity_mapping_name(lease_id, lease_nonce),
            lease_nonce: lease_nonce.into(),
            byte_length: 4 * 4 * 4,
            width: 4,
            height: 4,
            stride_bytes: 4 * 4,
            pixel_format: "b8g8r8a8_unorm".into(),
            content_sha256: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                .into(),
            source_asset_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .into(),
            source_media_type: "image/png".into(),
            expires_qpc: 9_000,
            qpc_frequency: 10_000_000,
            cancellation_generation: 3,
            capture_session_id: "identity-session".into(),
            selected_process_id: 123,
            selected_window_handle: 0x9988,
            selected_executable_name: "game.exe".into(),
            source_device_generation: 9,
            source_geometry_epoch: 7,
            picker_consent_token: request.picker_consent_token.clone(),
            game_profile_id: request.game_profile_id.clone(),
            character_id: request.character_id.clone(),
            subject_id: request.subject_id.clone(),
            reference_id: request.reference_id.clone(),
            subject_display_name: request.subject_display_name.clone(),
            source_class: match request.source_class {
                IdentityReferenceSourceClass::UserPrivate => {
                    IdentityReferenceSourceClassPayload::UserPrivate as i32
                }
                IdentityReferenceSourceClass::OriginalSynthetic => {
                    IdentityReferenceSourceClassPayload::OriginalSynthetic as i32
                }
            },
            owner_user_id: request.owner_user_id.clone().unwrap_or_default(),
            original_work_license: request.original_work_license.clone().unwrap_or_default(),
            explicit_user_consent: request.explicit_user_consent,
            local_only: request.local_only,
            imported_at_unix_ms: request.imported_at_unix_ms,
        }
    }

    #[test]
    fn identity_reference_rights_are_private_or_original_and_character_bound() {
        let private = private_reference_request();
        validate_identity_reference_import_request(&private).expect("private reference rights");

        let mut original = private.clone();
        original.source_class = IdentityReferenceSourceClass::OriginalSynthetic;
        original.owner_user_id = None;
        original.original_work_license = Some("Apache-2.0".into());
        original.explicit_user_consent = false;
        validate_identity_reference_import_request(&original).expect("original work rights");

        let mut mixed_rights = private.clone();
        mixed_rights.original_work_license = Some("Apache-2.0".into());
        assert!(validate_identity_reference_import_request(&mixed_rights).is_err());

        let mut cross_character = private.clone();
        cross_character.subject_id = "different-character".into();
        assert!(validate_identity_reference_import_request(&cross_character).is_err());

        let mut no_consent = private;
        no_consent.explicit_user_consent = false;
        assert!(validate_identity_reference_import_request(&no_consent).is_err());
    }

    #[test]
    fn identity_reference_lease_is_exactly_bound_and_capabilities_are_redacted() {
        assert_eq!(BrokerCommand::AllocateIdentityReferenceImport as i32, 21);
        assert_eq!(BrokerCommand::ReleaseIdentityReferenceImport as i32, 22);
        let request = private_reference_request();
        let payload = reference_lease_payload(&request);
        let lease =
            decode_identity_reference_import_lease(payload.clone(), &request, "identity-session")
                .expect("exact reference lease");
        assert_eq!(lease.character_id, "mara");
        assert_eq!(lease.source_media_type, "image/png");
        let debug = format!("{lease:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains(lease.lease_id()));
        assert!(!debug.contains(lease.lease_nonce()));
        assert!(!debug.contains(lease.shared_memory_name()));
        assert!(!debug.contains(&lease.content_sha256));
        assert!(!debug.contains(&lease.source_asset_sha256));

        let mut wrong_game = payload.clone();
        wrong_game.game_profile_id = "other-game".into();
        assert!(
            decode_identity_reference_import_lease(wrong_game, &request, "identity-session")
                .is_err()
        );

        let mut wrong_mapping = payload.clone();
        wrong_mapping.shared_memory_name =
            "Local\\npc.identity.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .into();
        assert!(decode_identity_reference_import_lease(
            wrong_mapping,
            &request,
            "identity-session"
        )
        .is_err());

        let mut wrong_rights = payload;
        wrong_rights.source_class = IdentityReferenceSourceClassPayload::OriginalSynthetic as i32;
        assert!(
            decode_identity_reference_import_lease(wrong_rights, &request, "identity-session")
                .is_err()
        );
    }

    #[test]
    fn playback_allocation_codec_and_secret_redaction_are_exact() {
        let request = AllocatePlaybackStreamPayload {
            session_id: "session-001".into(),
            turn_id: "turn-001".into(),
            generation: 7,
            sample_rate: 24_000,
            channels: 1,
            max_frames: 4_320_000,
            expected_producer_process_id: 42,
        };
        assert_eq!(
            AllocatePlaybackStreamPayload::decode(request.encode_to_vec().as_slice())
                .expect("allocation protobuf"),
            request
        );
        let payload = PlaybackLeasePayload {
            schema_version: 2,
            stream_id: "pcm-001".into(),
            producer_endpoint: r"\\.\pipe\npc-media-playback-session-pcm-001".into(),
            one_time_token: (1_u8..=32).collect(),
            session_id: request.session_id.clone(),
            turn_id: request.turn_id.clone(),
            generation: request.generation,
            sample_rate: request.sample_rate,
            channels: request.channels,
            max_frames: request.max_frames,
            max_chunk_bytes: 64 * 1024,
            expires_qpc: 99,
            output_selection_mode: AudioOutputSelectionModePayload::SystemDefault as i32,
            output_endpoint_id: "{0.0.0.00000000}.fixture-output".into(),
            output_endpoint_generation: 17,
        };
        let lease = decode_playback_lease(payload, &request).expect("bound lease");
        let debug = format!("{lease:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("01020304"));
        let wire = serde_json::to_value(&lease).expect("internal sidecar lease wire");
        assert_eq!(
            wire["oneTimeToken"],
            "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
        );
        assert_eq!(wire["maxChunkBytes"], 65_536);
        assert_eq!(wire["outputSelectionMode"], "systemDefault");
        assert_eq!(wire["outputEndpointGeneration"], 17);
    }

    #[test]
    fn playback_pool_request_enforces_sixteen_lease_and_duration_bounds() {
        let valid = AudioPlaybackPoolRequest {
            session_id: "session-001".into(),
            turn_id: "turn-001".into(),
            generation: 7,
            sample_rate: 44_100,
            channels: 1,
            max_frames_per_lease: 44_100 * 180,
            lease_count: MAX_PLAYBACK_LEASES_PER_TURN,
        };
        validate_playback_pool_request(&valid).expect("bounded request");
        assert!(validate_playback_pool_request(&AudioPlaybackPoolRequest {
            lease_count: MAX_PLAYBACK_LEASES_PER_TURN + 1,
            ..valid.clone()
        })
        .is_err());
        assert!(validate_playback_pool_request(&AudioPlaybackPoolRequest {
            max_frames_per_lease: 44_100 * 601,
            ..valid
        })
        .is_err());
    }

    #[test]
    fn visual_audio_envelope_requires_exact_causal_one_use_stream_evidence() {
        assert_eq!(BrokerCommand::QueryVisualAudioEnvelope as i32, 29);
        let query = VisualAudioEnvelopeQuery {
            session_id: "session-001".into(),
            turn_id: "turn-001".into(),
            generation: 7,
            stream_id: "pcm-001".into(),
            segment_id: "pcm-001".into(),
        };
        let payload = VisualAudioEnvelopePayload {
            schema_version: 1,
            session_id: query.session_id.clone(),
            turn_id: query.turn_id.clone(),
            generation: query.generation,
            stream_id: query.stream_id.clone(),
            segment_id: query.segment_id.clone(),
            source_sample_start: 240,
            source_sample_count: 240,
            sample_rate: 24_000,
            channels: 1,
            device_write_qpc: 500,
            qpc_frequency: 10_000_000,
            source_frames: 480,
            device_frames: 480,
            mono_rms_q15: (1..=8).collect(),
            mono_peak_q15: (1..=8).rev().collect(),
            active: true,
            draining: false,
            cancelled: false,
            visual_speech_cues: Vec::new(),
        };
        let decoded =
            decode_visual_audio_envelope(payload.clone(), &query).expect("exact causal envelope");
        assert_eq!(decoded.mono_rms_q15, [1, 2, 3, 4, 5, 6, 7, 8]);

        let mut wrong_stream = payload.clone();
        wrong_stream.stream_id = "pcm-other".into();
        assert!(decode_visual_audio_envelope(wrong_stream, &query).is_err());
        let mut cancelled = payload.clone();
        cancelled.cancelled = true;
        assert!(decode_visual_audio_envelope(cancelled, &query).is_err());
        let mut wrong_bins = payload;
        wrong_bins.mono_peak_q15.pop();
        assert!(decode_visual_audio_envelope(wrong_bins, &query).is_err());
    }

    #[test]
    fn visual_audio_envelope_decodes_only_bounded_ordered_non_overlapping_cues() {
        let query = VisualAudioEnvelopeQuery {
            session_id: "session-001".into(),
            turn_id: "turn-001".into(),
            generation: 7,
            stream_id: "pcm-001".into(),
            segment_id: "pcm-001".into(),
        };
        let valid_cue = VisualSpeechCuePayload {
            start_sample: 480,
            duration_samples: 240,
            canonical_viseme: 9,
            strength_q15: 30_000,
        };
        let payload_with = |cues: Vec<VisualSpeechCuePayload>| VisualAudioEnvelopePayload {
            schema_version: 1,
            session_id: query.session_id.clone(),
            turn_id: query.turn_id.clone(),
            generation: query.generation,
            stream_id: query.stream_id.clone(),
            segment_id: query.segment_id.clone(),
            source_sample_start: 240,
            source_sample_count: 240,
            sample_rate: 24_000,
            channels: 1,
            device_write_qpc: 500,
            qpc_frequency: 10_000_000,
            source_frames: 480,
            device_frames: 480,
            mono_rms_q15: vec![1; 8],
            mono_peak_q15: vec![2; 8],
            active: true,
            draining: false,
            cancelled: false,
            visual_speech_cues: cues,
        };

        let encoded = payload_with(vec![
            valid_cue.clone(),
            VisualSpeechCuePayload {
                start_sample: 720,
                duration_samples: 120,
                canonical_viseme: 1,
                strength_q15: 32_767,
            },
        ])
        .encode_to_vec();
        let decoded_payload = VisualAudioEnvelopePayload::decode(encoded.as_slice())
            .expect("prost tag 20 nested cue payload");
        let decoded =
            decode_visual_audio_envelope(decoded_payload, &query).expect("valid bounded cues");
        assert_eq!(
            decoded.visual_speech_cues,
            vec![
                VisualSpeechCue {
                    start_sample: 480,
                    duration_samples: 240,
                    canonical_viseme: 9,
                    strength_q15: 30_000,
                },
                VisualSpeechCue {
                    start_sample: 720,
                    duration_samples: 120,
                    canonical_viseme: 1,
                    strength_q15: 32_767,
                },
            ]
        );

        let invalid_cues = [
            VisualSpeechCuePayload {
                duration_samples: 0,
                ..valid_cue.clone()
            },
            VisualSpeechCuePayload {
                start_sample: u64::MAX,
                duration_samples: 1,
                ..valid_cue.clone()
            },
            VisualSpeechCuePayload {
                canonical_viseme: 11,
                ..valid_cue.clone()
            },
            VisualSpeechCuePayload {
                strength_q15: 32_768,
                ..valid_cue.clone()
            },
        ];
        for cue in invalid_cues {
            assert!(decode_visual_audio_envelope(payload_with(vec![cue]), &query).is_err());
        }
        assert!(decode_visual_audio_envelope(
            payload_with(vec![
                valid_cue.clone(),
                VisualSpeechCuePayload {
                    start_sample: 719,
                    duration_samples: 1,
                    ..valid_cue.clone()
                },
            ]),
            &query,
        )
        .is_err());
        assert!(decode_visual_audio_envelope(
            payload_with(vec![valid_cue; MAX_VISUAL_SPEECH_CUES + 1]),
            &query,
        )
        .is_err());
    }

    #[test]
    fn ptt_activation_baseline_accepts_only_a_strictly_newer_press() {
        assert_eq!(BrokerCommand::QueryPttActivationState as i32, 30);
        let baseline = decode_ptt_activation_state(PttActivationStatePayload {
            schema_version: 1,
            virtual_key: 0x77,
            state: 0,
            transition_sequence: 4,
            transition_qpc: 40_000,
            release_transition_sequence: 4,
            released_qpc: 40_000,
        })
        .expect("released arm baseline");
        let pressed = decode_ptt_activation_state(PttActivationStatePayload {
            schema_version: 1,
            virtual_key: 0x77,
            state: 1,
            transition_sequence: 5,
            transition_qpc: 50_000,
            release_transition_sequence: 4,
            released_qpc: 40_000,
        })
        .expect("newer press");
        assert!(pressed.is_strictly_newer_press_than(baseline));
        assert!(!baseline.is_strictly_newer_press_than(baseline));
        let wrong_key = PttActivationState {
            virtual_key: 0x76,
            ..pressed
        };
        assert!(!wrong_key.is_strictly_newer_press_than(baseline));
        assert!(decode_ptt_activation_state(PttActivationStatePayload {
            release_transition_sequence: 5,
            ..PttActivationStatePayload {
                schema_version: 1,
                virtual_key: 0x77,
                state: 1,
                transition_sequence: 5,
                transition_qpc: 50_000,
                release_transition_sequence: 4,
                released_qpc: 40_000,
            }
        })
        .is_err());
    }

    fn manual_actor_request_fixture() -> NativeManualActorPickerRequestV1 {
        NativeManualActorPickerRequestV1 {
            request_id: "native-actor-click-7".into(),
            visual_pack_id: "openseeface-mnv3-lm1-mouth-signal".into(),
            visual_pack_admission_sha256: "a".repeat(64),
            game_profile_id: "eclipse-harbor".into(),
            capture_session_id: "capture-session-7".into(),
            cancellation_generation: 9,
            selected_process_id: 42,
            selected_window_handle: 43,
            selected_executable_name: "synthetic-game.exe".into(),
            source_device_generation: 11,
            source_geometry_epoch: 12,
            source_frame_sequence: 13,
            source_frame_qpc: 14_000,
            qpc_frequency: 10_000_000,
            captured_at_unix_ms: 1_000,
            expires_at_unix_ms: 1_200,
            source_width: 1_920,
            source_height: 1_080,
            timeout_ms: 5_000,
            candidates: vec![
                NativeManualActorCandidateV1 {
                    actor_id: 101,
                    track_id: 201,
                    track_epoch: 301,
                    left: 0.1,
                    top: 0.2,
                    right: 0.3,
                    bottom: 0.7,
                },
                NativeManualActorCandidateV1 {
                    actor_id: 102,
                    track_id: 202,
                    track_epoch: 302,
                    left: 0.5,
                    top: 0.25,
                    right: 0.8,
                    bottom: 0.75,
                },
            ],
        }
    }

    fn manual_actor_receipt_payload(
        request: &NativeManualActorPickerRequestV1,
    ) -> ManualActorPickerReceiptPayload {
        ManualActorPickerReceiptPayload {
            schema_version: 1,
            request_id: request.request_id.clone(),
            status: 2,
            receipt_nonce_high: 0x1111,
            receipt_nonce_low: 0x2222,
            capture_session_id: request.capture_session_id.clone(),
            cancellation_generation: request.cancellation_generation,
            selected_process_id: request.selected_process_id,
            selected_window_handle: request.selected_window_handle,
            selected_executable_name: request.selected_executable_name.clone(),
            source_device_generation: request.source_device_generation,
            source_geometry_epoch: request.source_geometry_epoch,
            source_frame_sequence: request.source_frame_sequence,
            source_frame_qpc: request.source_frame_qpc,
            selected_actor_id: request.candidates[1].actor_id,
            selected_track_id: request.candidates[1].track_id,
            selected_track_epoch: request.candidates[1].track_epoch,
            candidate_count: request.candidates.len() as u32,
            candidate_set_sha256: manual_actor_candidate_set_sha256(&request.candidates),
            began_qpc: 15_000,
            clicked_qpc: 16_000,
            attested_at_qpc: 16_100,
            qpc_frequency: request.qpc_frequency,
            pointer_kind: 1,
            frozen_wgc_frame_verified: true,
            overlay_capture_excluded: true,
            overlay_nonactivating: true,
            single_hardware_pointer_click: true,
            pixels_withheld_from_webview: true,
            coordinates_withheld_from_webview: true,
        }
    }

    #[test]
    fn command31_codec_keeps_candidates_private_and_binds_exact_receipt() {
        assert_eq!(BrokerCommand::ManualActorPicker as i32, 31);
        let request = manual_actor_request_fixture();
        validate_manual_actor_picker_request(&request).expect("private request");
        let begin =
            manual_actor_picker_command_payload(ManualActorPickerActionPayload::Begin, &request);
        assert_eq!(begin.candidates.len(), 2);
        assert_eq!(begin.source_frame_sequence, request.source_frame_sequence);
        let poll =
            manual_actor_picker_command_payload(ManualActorPickerActionPayload::Poll, &request);
        assert!(poll.candidates.is_empty());
        assert_eq!(poll.source_frame_sequence, 0);

        let bytes = manual_actor_receipt_payload(&request).encode_to_vec();
        let receipt =
            decode_manual_actor_picker_receipt(&bytes, &request).expect("sealed selection");
        assert_eq!(receipt.status, NativeManualActorPickerStatusV1::Selected);
        assert_eq!(receipt.selected_actor_id, 102);
        assert!(valid_lower_sha256(&receipt.receipt_sha256));
        let debug = format!("{receipt:?}");
        assert!(!debug.contains(&request.request_id));
        assert!(!debug.contains(&request.selected_executable_name));
        assert!(!debug.contains(&receipt.candidate_set_sha256));

        let mut stale = manual_actor_receipt_payload(&request);
        stale.source_geometry_epoch += 1;
        assert!(decode_manual_actor_picker_receipt(&stale.encode_to_vec(), &request).is_err());
        let mut foreign = manual_actor_receipt_payload(&request);
        foreign.selected_actor_id = 999;
        assert!(decode_manual_actor_picker_receipt(&foreign.encode_to_vec(), &request).is_err());
    }

    #[test]
    fn command31_rejects_duplicate_candidates_and_unbound_poll_fields() {
        let mut duplicate = manual_actor_request_fixture();
        duplicate.candidates[1].actor_id = duplicate.candidates[0].actor_id;
        assert!(validate_manual_actor_picker_request(&duplicate).is_err());
        let request = manual_actor_request_fixture();
        let cancel =
            manual_actor_picker_command_payload(ManualActorPickerActionPayload::Cancel, &request);
        assert!(cancel.candidates.is_empty());
        assert_eq!(cancel.timeout_ms, 0);
        assert_eq!(cancel.source_device_generation, 0);
    }

    #[test]
    fn audio_output_selection_is_explicit_bounded_and_atomically_persisted() {
        let directory = tempfile::tempdir().expect("selection directory");
        let path = directory.path().join(AUDIO_OUTPUT_SELECTION_FILE_NAME);
        assert_eq!(
            load_audio_output_selection(&path).expect("missing selection"),
            None
        );

        persist_audio_output_selection(&path, &AudioOutputSelection::SystemDefault)
            .expect("persist explicit system default");
        assert_eq!(
            load_audio_output_selection(&path).expect("load system default"),
            Some(AudioOutputSelection::SystemDefault)
        );

        let endpoint_id = "{0.0.0.00000000}.{fixture-output}".to_string();
        let explicit = AudioOutputSelection::EndpointId {
            endpoint_id: endpoint_id.clone(),
        };
        persist_audio_output_selection(&path, &explicit).expect("persist endpoint ID");
        assert_eq!(
            load_audio_output_selection(&path).expect("load endpoint"),
            Some(explicit.clone())
        );
        assert!(
            encode_audio_output_selection(&AudioOutputSelection::EndpointId {
                endpoint_id: "x".repeat(1025),
            })
            .is_err()
        );

        let snapshot = decode_audio_output_snapshot(AudioOutputSnapshotPayload {
            schema_version: 1,
            catalog_generation: 9,
            endpoints: vec![AudioOutputEndpointPayload {
                endpoint_id: endpoint_id.clone(),
                friendly_name: "Fixture Speakers".into(),
                state: 1,
                system_default: true,
                generation: 17,
            }],
        })
        .expect("bounded endpoint snapshot");
        assert_eq!(snapshot.endpoints[0].endpoint_id, endpoint_id);
        assert_eq!(snapshot.endpoints[0].state, AudioOutputState::Active);
        let expected_rollback = SelectedAudioOutput {
            schema_version: 1,
            selection: AudioOutputSelection::SystemDefault,
            resolved: snapshot.endpoints[0].clone(),
        };
        let mut mismatched_rollback = expected_rollback.clone();
        mismatched_rollback.resolved.generation += 1;
        let mismatch: Result<SelectedAudioOutput, MediaBrokerError> = Ok(mismatched_rollback);
        assert_eq!(
            selection_rollback_status(&mismatch, &expected_rollback),
            "mismatched-success"
        );

        // A restart reads the last complete atomic record, and rollback can
        // restore both an exact prior selection and the first-run/no-file state.
        let previous = AudioOutputSelection::SystemDefault;
        persist_audio_output_selection(&path, &previous).expect("persist rollback baseline");
        persist_audio_output_selection(&path, &explicit).expect("persist candidate");
        restore_audio_output_selection(&path, Some(&previous)).expect("restore exact prior");
        assert_eq!(
            load_audio_output_selection(&path).expect("restart load after rollback"),
            Some(previous)
        );
        restore_audio_output_selection(&path, None).expect("restore first-run state");
        assert_eq!(
            load_audio_output_selection(&path).expect("restart no selection"),
            None
        );

        let impossible = directory.path().join("selection-is-a-directory");
        std::fs::create_dir(&impossible).expect("create persistence failure target");
        assert!(persist_audio_output_selection(&impossible, &explicit).is_err());
        assert!(restore_audio_output_selection(&impossible, None).is_err());
    }

    #[test]
    fn input_selection_and_native_lease_are_bounded_persisted_and_secret_redacted() {
        assert_eq!(BrokerCommand::TrustedSubtitlePresentationContext as i32, 23);
        assert_eq!(BrokerCommand::EnumerateAudioInputs as i32, 24);
        assert_eq!(BrokerCommand::CancelAudioInputRehearsal as i32, 28);
        let directory = tempfile::tempdir().expect("input selection directory");
        let output_path = directory.path().join(AUDIO_OUTPUT_SELECTION_FILE_NAME);
        let path = audio_input_selection_path(&output_path).expect("input selection path");
        assert_eq!(
            path.file_name().and_then(|value| value.to_str()),
            Some(AUDIO_INPUT_SELECTION_FILE_NAME)
        );
        assert_eq!(
            load_audio_input_selection(&path).expect("missing input selection"),
            None
        );
        persist_audio_input_selection(&path, &AudioInputSelection::SystemDefault)
            .expect("persist system default input");
        assert_eq!(
            load_audio_input_selection(&path).expect("load input selection"),
            Some(AudioInputSelection::SystemDefault)
        );

        let endpoint_id = r"{0.0.1.00000000}.{fixture-input}".to_owned();
        let explicit = AudioInputSelection::EndpointId {
            endpoint_id: endpoint_id.clone(),
        };
        persist_audio_input_selection(&path, &explicit).expect("persist exact input");
        assert_eq!(
            load_audio_input_selection(&path).expect("load exact input"),
            Some(explicit)
        );

        restore_audio_input_selection(&path, None).expect("restore no input selection");
        assert_eq!(
            load_audio_input_selection(&path).expect("restart input state"),
            None
        );
        let expected_rollback = SelectedAudioInput {
            schema_version: 1,
            selection: AudioInputSelection::SystemDefault,
            resolved: AudioInputEndpoint {
                endpoint_id: endpoint_id.clone(),
                friendly_name: "Fixture Microphone".into(),
                state: AudioInputState::Active,
                system_default: true,
                generation: 17,
            },
        };
        let mut mismatched_rollback = expected_rollback.clone();
        mismatched_rollback.resolved.endpoint_id.push_str("-wrong");
        let mismatch: Result<SelectedAudioInput, MediaBrokerError> = Ok(mismatched_rollback);
        assert_eq!(
            selection_rollback_status(&mismatch, &expected_rollback),
            "mismatched-success"
        );
        let impossible = directory.path().join("input-selection-is-a-directory");
        std::fs::create_dir(&impossible).expect("create input persistence failure target");
        assert!(
            persist_audio_input_selection(&impossible, &AudioInputSelection::SystemDefault)
                .is_err()
        );
        assert!(restore_audio_input_selection(&impossible, None).is_err());

        let request = AllocateAudioInputRehearsalPayload {
            session_id: "session-001".into(),
            turn_id: "turn-001".into(),
            generation: 7,
            duration_ms: 500,
            sample_rate: 16_000,
            channels: 1,
            max_frames: 8_000,
            expected_producer_process_id: 4242,
            activation_source: InputActivationSourcePayload::PushToTalk as i32,
        };
        let payload = AudioInputRehearsalLeasePayload {
            schema_version: 2,
            stream_id: "mic-stream-001".into(),
            producer_endpoint: r"\\.\pipe\npc-media-input-fixture".into(),
            one_time_token: vec![0x2a; 32],
            session_id: request.session_id.clone(),
            turn_id: request.turn_id.clone(),
            generation: request.generation,
            duration_ms: request.duration_ms,
            sample_rate: request.sample_rate,
            channels: request.channels,
            max_frames: request.max_frames,
            max_chunk_bytes: 64 * 1024,
            expires_qpc: 20_000,
            qpc_frequency: 10_000_000,
            input_selection_mode: AudioOutputSelectionModePayload::EndpointId as i32,
            input_endpoint_id: endpoint_id.clone(),
            input_endpoint_generation: 17,
            activation_source: InputActivationSourcePayload::PushToTalk as i32,
            ptt_virtual_key: 0x77,
            ptt_press_transition_sequence: 8,
            ptt_pressed_qpc: 12_000,
        };
        let lease =
            decode_audio_input_rehearsal_lease(payload, &request).expect("strict input lease");
        assert_eq!(lease.turn_id, request.turn_id);
        assert_eq!(lease.input_endpoint_id, endpoint_id);
        let debug = format!("{lease:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("2a2a2a2a"));
        let serialized = serde_json::to_string(&lease).expect("serialize native handoff lease");
        assert!(serialized.contains("2a2a2a2a"));
        assert!(!serialized.contains("one_time_token"));

        let valid = AudioInputRehearsalRequest {
            session_id: "session-001".into(),
            turn_id: "turn-001".into(),
            generation: 7,
            duration_ms: 500,
            sample_rate: 16_000,
            channels: 1,
            max_frames: 8_000,
            activation_source: InputActivationSource::PushToTalk,
        };
        validate_audio_input_rehearsal_request(&valid).expect("bounded rehearsal");
        assert!(
            validate_audio_input_rehearsal_request(&AudioInputRehearsalRequest {
                duration_ms: MAX_INPUT_REHEARSAL_DURATION_MS + 1,
                ..valid
            })
            .is_err()
        );
    }

    #[test]
    fn trusted_subtitle_context_requires_exact_identity_display_and_fresh_attestation() {
        let payload = TrustedSubtitlePresentationContextPayload {
            schema_version: 1,
            selected_process_id: 4242,
            selected_window: 0x1234,
            capture_device_generation: 8,
            geometry_epoch: 13,
            source_frame_sequence: 21,
            source_frame_qpc: 10_000,
            window_left_px: -1920,
            window_top_px: 0,
            window_right_px: 0,
            window_bottom_px: 1080,
            client_left_px: -1912,
            client_top_px: 31,
            client_right_px: -8,
            client_bottom_px: 1072,
            captured_width_px: 1904,
            captured_height_px: 1041,
            monitor_id: r"\\.\DISPLAY2".into(),
            monitor_left_px: -1920,
            monitor_top_px: 0,
            monitor_right_px: 0,
            monitor_bottom_px: 1080,
            work_left_px: -1920,
            work_top_px: 0,
            work_right_px: 0,
            work_bottom_px: 1040,
            dpi_available: true,
            dpi_x: 144,
            dpi_y: 144,
            hdr_evidence_available: true,
            hdr_supported: true,
            hdr_user_enabled: true,
            hdr_active: true,
            advanced_color_active: true,
            active_color_mode: 2,
            color_encoding_available: true,
            color_encoding: 0,
            bits_per_color_channel: 10,
            sdr_white_level_available: true,
            sdr_white_level_nits: 203.2,
            capture_backend: 1,
            capture_scope: 1,
            overlay_capture_excluded: true,
            overlay_visuals_allowed: true,
            selected_executable_name: "Game.ExE".into(),
            target_color_space_available: true,
            target_color_space: 3,
            attested_at_qpc: 10_050,
            qpc_frequency: 10_000_000,
            attestation_id: 99,
        };
        let context = decode_trusted_subtitle_presentation_context(payload.clone())
            .expect("strict trusted subtitle context");
        assert_eq!(context.capture_scope, TrustedCaptureScope::ExactGameHwndWgc);
        assert_eq!(
            context.target_color_space,
            Some(TrustedTargetColorSpace::HdrScRgb)
        );
        assert_eq!(context.selected_executable_name, "Game.ExE");

        let mut stale = payload.clone();
        stale.attested_at_qpc = stale.source_frame_qpc + stale.qpc_frequency * 3;
        assert!(decode_trusted_subtitle_presentation_context(stale).is_err());
        let mut mismatched_scope = payload.clone();
        mismatched_scope.capture_scope = 2;
        assert!(decode_trusted_subtitle_presentation_context(mismatched_scope).is_err());
        let mut guessed_dpi = payload;
        guessed_dpi.dpi_available = false;
        assert!(decode_trusted_subtitle_presentation_context(guessed_dpi).is_err());
    }

    #[test]
    fn target_mutations_accept_only_one_monotonic_generation_step() {
        let wire = BrokerResponse {
            version: PROTOCOL_VERSION,
            response_to_sequence: 2,
            status: BrokerStatus::Ok as i32,
            cancellation_generation: 1,
            payload: Vec::new(),
        }
        .encode_to_vec();
        let decoded = BrokerResponse::decode(wire.as_slice()).expect("decode native response");
        assert_eq!(
            reconcile_response_generation(
                BrokerCommand::SelectTarget,
                0,
                decoded.cancellation_generation,
                decoded.status(),
            )
            .expect("post-command generation"),
            1
        );
        assert_eq!(
            reconcile_response_generation(BrokerCommand::SelectTarget, 0, 0, BrokerStatus::Ok)
                .expect("legacy pre-command response"),
            1
        );
        assert_eq!(
            reconcile_response_generation(BrokerCommand::ClearTarget, 7, 8, BrokerStatus::Ok)
                .expect("clear transition"),
            8
        );
        assert!(
            reconcile_response_generation(BrokerCommand::SelectTarget, 7, 9, BrokerStatus::Ok)
                .is_err()
        );
        assert!(
            reconcile_response_generation(BrokerCommand::SelectTarget, 7, 6, BrokerStatus::Ok)
                .is_err()
        );
        assert!(
            reconcile_response_generation(BrokerCommand::Health, 7, 8, BrokerStatus::Ok).is_err()
        );
    }

    #[test]
    fn synthetic_target_payload_matches_native_select_target_contract() {
        let payload = SelectTargetPayload {
            native_window: 0x1234,
            expected_process_id: 77,
            allowed_process_names: vec![DEBUG_SYNTHETIC_TARGET_BASENAME.into()],
        };
        let encoded = payload.encode_to_vec();
        let decoded = SelectTargetPayload::decode(encoded.as_slice()).expect("decode payload");
        assert_eq!(decoded, payload);
        assert_eq!(decoded.allowed_process_names.len(), 1);
    }

    #[test]
    fn capture_evidence_decoder_requires_exact_bound_identity_shape() {
        let name = b"interactive-npcs-synthetic-target.exe";
        let mut payload = Vec::new();
        for value in [3_u32, 77] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0x1234_u64, 3, 4, 12, 99, 0x1111, 0x2222, 2, 1, 0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1280_u32, 720, 1, 1, name.len() as u32] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1_u32, 1, 1, 1, 1] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(name);
        let evidence = decode_native_capture_evidence(&payload).expect("exact capture evidence");
        assert_eq!(evidence.selected_process_id, 77);
        assert_eq!(evidence.selected_window_handle, 0x1234);
        assert_eq!(evidence.latest_frame_sequence, 12);
        assert!(evidence.external_display_overlays_may_change_perceived_brightness);
        assert_eq!(
            evidence.selected_executable_name,
            "interactive-npcs-synthetic-target.exe"
        );

        let mut invalid_boolean = payload.clone();
        invalid_boolean[96..100].copy_from_slice(&2_u32.to_le_bytes());
        assert!(decode_native_capture_evidence(&invalid_boolean).is_err());
        let mut path_name = payload;
        let replacement = b"C:\\synthetic-target.exe";
        path_name.truncate(104);
        path_name.extend_from_slice(&(replacement.len() as u32).to_le_bytes());
        path_name.extend_from_slice(&1_u32.to_le_bytes());
        path_name.extend_from_slice(&1_u32.to_le_bytes());
        path_name.extend_from_slice(&1_u32.to_le_bytes());
        path_name.extend_from_slice(&1_u32.to_le_bytes());
        path_name.extend_from_slice(&1_u32.to_le_bytes());
        path_name.extend_from_slice(replacement);
        assert!(decode_native_capture_evidence(&path_name).is_err());
    }

    #[test]
    fn synthetic_target_gate_accepts_only_the_task_owned_executable() {
        let accepted = validate_debug_synthetic_target(
            0x1234,
            77,
            "interactive-npcs-synthetic-target.exe",
            DEBUG_SYNTHETIC_PORTRAIT_SHA256,
            DebugSyntheticQualificationMode::StaticCompatibility,
        )
        .expect("dedicated fixture accepted");
        assert_eq!(accepted.native_window, 0x1234);
        assert_eq!(accepted.process_id, 77);
        assert_eq!(
            accepted.executable_basename,
            DEBUG_SYNTHETIC_TARGET_BASENAME
        );

        assert!(validate_debug_synthetic_target(
            0,
            77,
            DEBUG_SYNTHETIC_TARGET_BASENAME,
            DEBUG_SYNTHETIC_PORTRAIT_SHA256,
            DebugSyntheticQualificationMode::StaticCompatibility,
        )
        .is_err());
        assert!(validate_debug_synthetic_target(
            0x1234,
            0,
            DEBUG_SYNTHETIC_TARGET_BASENAME,
            DEBUG_SYNTHETIC_PORTRAIT_SHA256,
            DebugSyntheticQualificationMode::StaticCompatibility,
        )
        .is_err());
        assert!(validate_debug_synthetic_target(
            0x1234,
            77,
            "Cyberpunk2077.exe",
            DEBUG_SYNTHETIC_PORTRAIT_SHA256,
            DebugSyntheticQualificationMode::StaticCompatibility,
        )
        .is_err());
        assert!(validate_debug_synthetic_target(
            0x1234,
            77,
            "C:\\fixtures\\interactive-npcs-synthetic-target.exe",
            DEBUG_SYNTHETIC_PORTRAIT_SHA256,
            DebugSyntheticQualificationMode::StaticCompatibility,
        )
        .is_err());
        assert!(validate_debug_synthetic_target(
            0x1234,
            77,
            DEBUG_SYNTHETIC_TARGET_BASENAME,
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            DebugSyntheticQualificationMode::StaticCompatibility,
        )
        .is_err());
    }

    #[test]
    fn synthetic_metadata_requires_ready_consistent_task_owned_identity() {
        let fixture = serde_json::json!({
            "schema_version": 1,
            "fixture_kind": "synthetic-original-video-replay",
            "state": "playing",
            "pid": 77,
            "process_id": 77,
            "window_handle": 4660,
            "hwnd": 4660,
            "executable_basename": DEBUG_SYNTHETIC_TARGET_BASENAME,
            "exe_basename": DEBUG_SYNTHETIC_TARGET_BASENAME,
            "decoded_frames": 3,
            "visual_source": DEBUG_SYNTHETIC_VISUAL_SOURCE,
            "portrait_sha256": DEBUG_SYNTHETIC_PORTRAIT_SHA256,
            "source_mouth_motion": false,
            "untrusted_extra_field": "ignored"
        });
        let accepted = decode_debug_synthetic_target_metadata(
            &serde_json::to_vec(&fixture).expect("serialize metadata"),
        )
        .expect("valid task metadata");
        assert_eq!(accepted.native_window, 4660);
        assert_eq!(accepted.process_id, 77);

        for (field, replacement) in [
            ("state", serde_json::json!("ready")),
            ("decoded_frames", serde_json::json!(0)),
            ("pid", serde_json::json!(78)),
            ("hwnd", serde_json::json!(4661)),
            ("window_handle", serde_json::json!(-1)),
            ("executable_basename", serde_json::json!("notepad.exe")),
            ("portrait_sha256", serde_json::json!("bad")),
            ("source_mouth_motion", serde_json::json!(true)),
            (
                "exe_basename",
                serde_json::json!("C:\\fixtures\\interactive-npcs-synthetic-target.exe"),
            ),
        ] {
            let mut invalid = fixture.clone();
            invalid[field] = replacement;
            assert!(decode_debug_synthetic_target_metadata(
                &serde_json::to_vec(&invalid).expect("serialize invalid metadata")
            )
            .is_err());
        }

        let directory = tempfile::tempdir().expect("metadata directory");
        let fixed_path = directory.path().join(DEBUG_SYNTHETIC_METADATA_FILE_NAME);
        std::fs::write(
            &fixed_path,
            serde_json::to_vec(&fixture).expect("serialize fixed metadata"),
        )
        .expect("write fixed metadata");
        assert!(read_debug_synthetic_target(&fixed_path).is_ok());
        assert!(
            read_debug_synthetic_target(&directory.path().join("capture-target.json")).is_err()
        );
    }

    #[test]
    fn moving_synthetic_metadata_requires_the_exact_attested_source_and_rendered_motion() {
        let fixture = serde_json::json!({
            "schema_version": 2,
            "fixture_kind": "synthetic-original-video-replay",
            "state": "playing",
            "pid": 77,
            "process_id": 77,
            "window_handle": 4660,
            "hwnd": 4660,
            "executable_basename": DEBUG_SYNTHETIC_TARGET_BASENAME,
            "exe_basename": DEBUG_SYNTHETIC_TARGET_BASENAME,
            "decoded_frames": 3,
            "visual_source": DEBUG_SYNTHETIC_MOVING_VISUAL_SOURCE,
            "portrait_sha256": DEBUG_SYNTHETIC_PORTRAIT_SHA256,
            "source_sequence_sha256": DEBUG_SYNTHETIC_SOURCE_SEQUENCE_SHA256,
            "source_frame_count": 42,
            "source_frame_width": 960,
            "source_frame_height": 720,
            "source_frame_rate": 30,
            "source_frame_motion": true,
            "source_actor_motion": false,
            "rendered_actor_motion": true,
            "rendered_blink_motion": true,
            "rendered_breathing_motion": true,
            "source_mouth_articulation": false,
            "product_lip_sync": false,
            "visual_mode": "moving-source-controlled-idle-v2",
            "content_frame_index": 7,
            "content_frame_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        });
        let accepted = decode_debug_synthetic_target_metadata(
            &serde_json::to_vec(&fixture).expect("serialize moving metadata"),
        )
        .expect("exact moving metadata is accepted");
        assert_eq!(
            accepted.qualification_mode,
            DebugSyntheticQualificationMode::MovingSourceControlledIdle
        );

        for (field, replacement) in [
            ("source_sequence_sha256", serde_json::json!("bad")),
            ("source_frame_count", serde_json::json!(41)),
            ("source_frame_width", serde_json::json!(1280)),
            ("source_frame_height", serde_json::json!(719)),
            ("source_frame_rate", serde_json::json!(60)),
            ("source_frame_motion", serde_json::json!(false)),
            ("source_actor_motion", serde_json::json!(true)),
            ("rendered_actor_motion", serde_json::json!(false)),
            ("rendered_blink_motion", serde_json::json!(false)),
            ("rendered_breathing_motion", serde_json::json!(false)),
            ("source_mouth_articulation", serde_json::json!(true)),
            ("product_lip_sync", serde_json::json!(true)),
            ("content_frame_index", serde_json::json!(42)),
            (
                "content_frame_sha256",
                serde_json::json!(
                    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                ),
            ),
        ] {
            let mut invalid = fixture.clone();
            invalid[field] = replacement;
            assert!(
                decode_debug_synthetic_target_metadata(
                    &serde_json::to_vec(&invalid).expect("serialize invalid moving metadata")
                )
                .is_err(),
                "field {field} must fail closed"
            );
        }
    }

    #[test]
    fn diagnostics_decoder_rejects_unbounded_or_truncated_payloads() {
        assert!(decode_diagnostics(&[]).is_err());
        assert!(decode_diagnostics(&[0; 79]).is_err());
        assert!(decode_diagnostics(&[0; 81]).is_err());
        assert!(decode_diagnostics(&[0; 103]).is_err());
        assert!(decode_diagnostics(&[0; 80]).is_ok());
        assert!(decode_diagnostics(&[0; 104]).is_ok());
    }

    #[test]
    fn missing_source_dev_broker_is_explicit_fixture_skip() {
        let runtime = RuntimeSupervisor::try_new(crate::sidecar_supervisor::RuntimeLaunchConfig {
            executable: PathBuf::from("C:/missing/npc-runtime.exe"),
            resource_root: PathBuf::from("C:/missing/resources"),
            app_data: PathBuf::from("C:/missing/data"),
            development_fixture_allowed: true,
            application_namespace:
                interactive_npcs_credential_vault::PRODUCTION_APPLICATION_NAMESPACE.into(),
        })
        .expect("parent job");
        let supervisor = MediaBrokerSupervisor::new(
            MediaBrokerLaunchConfig {
                executable: PathBuf::from("C:/missing/npc-media-broker.exe"),
                development_fixture_allowed: true,
                audio_output_selection_path: PathBuf::from(
                    "C:/missing/audio-output-selection-v1.json",
                ),
                debug_synthetic_metadata_path: PathBuf::from(
                    "C:/missing/debug-synthetic-replay-target.json",
                ),
            },
            runtime,
        );
        let health = supervisor.health();
        assert_eq!(health.state, RuntimeConnectionState::DevelopmentFixture);
        assert!(health.fixture_only);
        assert!(!health.connected);
    }
}
