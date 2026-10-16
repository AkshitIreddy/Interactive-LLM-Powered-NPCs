//! Rust half of the Rust/C++ mouth-worker wire contract.
//!
//! The native decoder (`native/mouth-worker/src/service_protocol.cpp`) and this
//! module's encoder/decoder were once changed independently, which made every
//! frame of a practice-game turn fail before it reached the presentation stage.
//! These tests pin each side of the contract and exchange committed golden byte
//! fixtures with `native/mouth-worker/tests/service_protocol_tests.cpp`:
//!
//! * `rust-admitted-render-command-v1.bin` is written by the Rust encoder and
//!   decoded by the C++ test.
//! * `cpp-residual-schema3-response-v1.bin` is written by the C++ encoder and
//!   decoded by the Rust test below.
//!
//! Regenerate the file a side owns with `NPC_REGENERATE_WIRE_FIXTURES=1`.

#![allow(clippy::unwrap_used)]

use super::*;

const CAPTURE_GENERATION: u64 = 42;
const SEGMENT_ID: u64 = 0x1122_3344_5566_7788;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../native/mouth-worker/protocol/fixtures")
}

fn regenerating() -> bool {
    std::env::var_os("NPC_REGENERATE_WIRE_FIXTURES").is_some()
}

fn test_envelope(generation: u64) -> VisualAudioEnvelope {
    VisualAudioEnvelope {
        schema_version: 1,
        session_id: "session-1".into(),
        turn_id: "turn-1".into(),
        generation,
        stream_id: "pcm-001".into(),
        segment_id: "pcm-001".into(),
        source_sample_start: 960,
        source_sample_count: 480,
        sample_rate: 48_000,
        channels: 1,
        device_write_qpc: 10_000,
        qpc_frequency: 10_000_000,
        source_frames: 1_440,
        device_frames: 1_440,
        mono_rms_q15: [1_024, 2_048, 3_072, 4_096, 5_120, 6_144, 7_168, 8_192],
        mono_peak_q15: [2_048, 4_096, 6_144, 8_192, 10_240, 12_288, 14_336, 16_384],
        active: true,
        draining: false,
        cancelled: false,
        visual_speech_cues: Vec::new(),
    }
}

fn drive_clock(drive: &MouthDrive) -> (u64, u64, u64, u64, u64) {
    match drive {
        MouthDrive::CausalEnvelopeCoefficients {
            stream_generation,
            segment_id,
            first_sample_index,
            sample_count,
            playback_sample_index,
            ..
        }
        | MouthDrive::TimedViseme {
            stream_generation,
            segment_id,
            first_sample_index,
            sample_count,
            playback_sample_index,
            ..
        }
        | MouthDrive::PcmWindow {
            stream_generation,
            segment_id,
            first_sample_index,
            sample_count,
            playback_sample_index,
            ..
        } => (
            *stream_generation,
            *segment_id,
            *first_sample_index,
            *sample_count,
            *playback_sample_index,
        ),
    }
}

#[test]
fn drive_carries_the_broker_capture_generation_not_the_turn_counter() {
    let envelope = test_envelope(7);
    let drive = drive_from_visual_audio_envelope(&envelope, 1_500_000, CAPTURE_GENERATION)
        .expect("causal drive");
    let (stream_generation, _, _, _, _) = drive_clock(&drive);
    assert_eq!(
        stream_generation, CAPTURE_GENERATION,
        "the worker compares the drive generation with the track's capture generation"
    );
    assert_ne!(stream_generation, envelope.generation);

    assert!(matches!(
        drive_from_visual_audio_envelope(&envelope, 1_500_000, 0),
        Err(VisualRuntimeError::NoSelectedTarget)
    ));
}

#[test]
fn drive_playback_sample_stays_inside_its_declared_span_at_every_instant() {
    let mut envelope = test_envelope(7);
    // 480 source samples are not divisible by every bin boundary a 48 kHz
    // clock can land on once a viseme cue is added, so sweep the whole block.
    for with_cue in [false, true] {
        envelope.visual_speech_cues = if with_cue {
            vec![crate::media_broker::VisualSpeechCue {
                start_sample: 1_000,
                duration_samples: 200,
                canonical_viseme: 9,
                strength_q15: 30_000,
            }]
        } else {
            Vec::new()
        };
        for render_at_ns in (1_000_000..=10_900_000).step_by(37_000) {
            let drive =
                drive_from_visual_audio_envelope(&envelope, render_at_ns, CAPTURE_GENERATION)
                    .expect("drive inside the block");
            let (_, segment_id, first, count, playback) = drive_clock(&drive);
            assert_ne!(segment_id, 0);
            assert!(count > 0, "an empty span is rejected by the worker");
            assert!(
                (first..first + count).contains(&playback),
                "render at {render_at_ns} ns: playback sample {playback} outside {first}+{count}"
            );
            assert!(drive_sample_span_contains_playback(first, count, playback));
        }
    }
}

#[test]
fn only_a_live_well_formed_envelope_passes_the_cheap_liveness_gate() {
    let live = test_envelope(7);
    assert!(visual_audio_envelope_is_presentable(&live));
    let mut idle = live.clone();
    idle.active = false;
    assert!(!visual_audio_envelope_is_presentable(&idle));
    let mut draining = live.clone();
    draining.draining = true;
    assert!(!visual_audio_envelope_is_presentable(&draining));
    let mut cancelled = live.clone();
    cancelled.cancelled = true;
    assert!(!visual_audio_envelope_is_presentable(&cancelled));
    let mut mismatched = live.clone();
    mismatched.segment_id = "other".into();
    assert!(!visual_audio_envelope_is_presentable(&mismatched));
    let mut loud = live;
    loud.mono_peak_q15[3] = 40_000;
    assert!(!visual_audio_envelope_is_presentable(&loud));
}

#[test]
fn drive_with_a_playback_sample_outside_its_span_is_rejected_before_the_worker() {
    let mut request = golden_request();
    assert!(validate_admitted_frame_request(&request).is_ok());
    for (first, count, playback) in [(960_u64, 60_u64, 1_020_u64), (960, 60, 959), (960, 0, 960)] {
        let drive = MouthDrive::CausalEnvelopeCoefficients {
            stream_generation: CAPTURE_GENERATION,
            segment_id: SEGMENT_ID,
            first_sample_index: first,
            sample_count: count,
            playback_sample_index: playback,
            sample_rate: 48_000,
            channels: 1,
            playback_at_ns: 1,
            coefficients: [0.0; 8],
        };
        let frame = VisualFrameRequest {
            drive,
            ..plain_frame_request()
        };
        assert!(
            matches!(
                validate_frame_request(&frame),
                Err(VisualRuntimeError::InvalidRequest)
            ),
            "span {first}+{count} must not contain {playback}"
        );
    }
    request.audio.generation = 0;
    assert!(validate_admitted_frame_request(&request).is_err());
}

fn plain_frame_request() -> VisualFrameRequest {
    VisualFrameRequest {
        request_id: 1,
        session_id_high: 1,
        session_id_low: 2,
        turn_id_high: 3,
        turn_id_low: 4,
        sentence_id: 1,
        track: VisualTrackBinding {
            actor_id: 5,
            track_id: 6,
            track_epoch: 7,
        },
        landmarks: OpenSeeFacePacket {
            provider_instance_id: 8,
            expected_frame_sequence: 9,
            face_x: 0.2,
            face_y: 0.2,
            face_width: 0.4,
            face_height: 0.5,
            landmarks: std::array::from_fn(|_| NormalizedLandmark {
                x: 0.4,
                y: 0.5,
                confidence: 0.99,
            }),
            yaw_degrees: 0.0,
            pitch_degrees: 0.0,
            roll_degrees: 0.0,
            detector_confidence: 0.99,
            landmark_confidence: 0.99,
            visibility_ratio: 0.99,
            mouth_occluded: false,
            measured_at_ns: 1,
        },
        appearance: golden_request().appearance,
        pressure: VisualPressure::Nominal,
        admitted_signal_rate_hz: 15,
        local_visuals_admitted: true,
        drive: golden_drive(),
        deadline_ns: 2,
    }
}

#[test]
fn drive_wire_follows_the_native_field_order_with_the_sample_span() {
    let envelope = test_envelope(7);
    let drive = drive_from_visual_audio_envelope(&envelope, 1_500_000, CAPTURE_GENERATION)
        .expect("causal drive");
    let mut writer = WireWriter::default();
    write_drive(&mut writer, &drive);
    let bytes = writer.take();
    // kind u8, generation/segment/first/count/playback u64 x5, rate u32,
    // channels u16, playback_at i64, 8 x f64, viseme u8, strength f64, pcm u32.
    assert_eq!(bytes.len(), 1 + 5 * 8 + 4 + 2 + 8 + 8 * 8 + 1 + 8 + 4);
    let mut reader = WireReader::new(&bytes);
    assert_eq!(reader.u8().unwrap(), 0, "causal envelope coefficient kind");
    assert_eq!(reader.u64().unwrap(), CAPTURE_GENERATION);
    assert_eq!(reader.u64().unwrap(), stable_nonzero_id("pcm-001"));
    assert_eq!(reader.u64().unwrap(), 960, "first_sample_index");
    assert_eq!(reader.u64().unwrap(), 60, "sample_count");
    assert_eq!(reader.u64().unwrap(), 984, "playback_sample_index");
    assert_eq!(reader.u32().unwrap(), 48_000);
    assert_eq!(reader.u16().unwrap(), 1);
    assert_eq!(reader.i64().unwrap(), 1_500_000);
    for _ in 0..8 {
        let coefficient = reader.f64().unwrap();
        assert!((0.0..=1.0).contains(&coefficient));
    }
    assert_eq!(reader.u8().unwrap(), 0);
    assert_eq!(reader.f64().unwrap(), 1.0);
    assert_eq!(
        reader.u32().unwrap(),
        0,
        "no PCM samples ride the envelope drive"
    );
    assert!(reader.done());
}

fn golden_session() -> SessionBinding {
    SessionBinding {
        nonce: std::array::from_fn(|index| index as u8 + 1),
        high: 101,
        low: 102,
    }
}

fn golden_lease() -> VisualSourceLease {
    VisualSourceLease {
        schema_version: 1,
        broker_process_id: 50,
        broker_process_creation_time: 600,
        broker_executable_name: "npc-media-broker.exe".into(),
        worker: VisualWorkerIdentity {
            process_id: 60,
            process_creation_time: 700,
            executable_name: "npc-mouth-worker.exe".into(),
        },
        worker_handle_value: 0x1234,
        lease_nonce_high: 401,
        lease_nonce_low: 402,
        adapter_luid: (2_u64 << 32) | 77,
        keyed_mutex_acquire_key: 1,
        keyed_mutex_release_key: 2,
        width: 1_920,
        height: 1_080,
        stride_bytes: 1_920 * 4,
        dxgi_format: 87,
        alpha_mode: 1,
        expires_qpc: 10_800_000,
        qpc_frequency: 10_000_000,
        cancellation_generation: CAPTURE_GENERATION,
        source_device_generation: 12,
        source_geometry_epoch: 13,
        source_frame_sequence: 11,
        source_frame_qpc: 9_000_000,
        track: VisualTrackBinding {
            actor_id: 8,
            track_id: 9,
            track_epoch: 10,
        },
    }
}

fn golden_request() -> AdmittedVisualFrameRequest {
    AdmittedVisualFrameRequest {
        request_id: 1,
        session_id_high: 101,
        session_id_low: 102,
        turn_id_high: 201,
        turn_id_low: 202,
        sentence_id: 301,
        game_profile_id: "eclipse-harbor".into(),
        character_id: "mara-venn".into(),
        selected_process_id: 42,
        selected_window_handle: 43,
        selected_executable_name: "interactive-npcs-synthetic-target.exe".into(),
        track: golden_lease().track,
        cancellation_generation: CAPTURE_GENERATION,
        expected_frame_sequence: 11,
        expected_frame_qpc: 9_000_000,
        qpc_frequency: 10_000_000,
        expected_device_generation: 12,
        expected_geometry_epoch: 13,
        expected_source_width: 1_920,
        expected_source_height: 1_080,
        seed_face_x: 0.2,
        seed_face_y: 0.1,
        seed_face_width: 0.4,
        seed_face_height: 0.7,
        appearance: AppearanceGateEvidence {
            descriptor_revision: 3,
            expected_digest_high: 21,
            expected_digest_low: 22,
            observed_digest_high: 23,
            observed_digest_low: 24,
            similarity: 0.93,
            temporal_iou: 0.87,
            blocker_coverage: 0.04,
            identity_locked: true,
            target_visible: true,
            scene_transition: false,
        },
        pressure: VisualPressure::ElevatedCpu,
        admitted_signal_rate_hz: 10,
        audio: VisualAudioBinding {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            generation: 7,
            stream_id: "pcm-001".into(),
        },
        deadline_ns: 1_070_000_000,
        sealed_click_source_only: false,
        user_assigned_character_pack: false,
        assigned_pack_content_sha256: None,
        assigned_pack_enrollment_sha256: None,
        assigned_pack_identity_revision: None,
    }
}

fn golden_drive() -> MouthDrive {
    MouthDrive::CausalEnvelopeCoefficients {
        stream_generation: CAPTURE_GENERATION,
        segment_id: SEGMENT_ID,
        first_sample_index: 960,
        sample_count: 60,
        playback_sample_index: 984,
        sample_rate: 48_000,
        channels: 1,
        playback_at_ns: 1_000_500_000,
        coefficients: [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0],
    }
}

/// The values the C++ test asserts after decoding this fixture are written out
/// in `test_rust_admitted_render_fixture_decodes` and must stay in sync.
#[test]
fn admitted_render_command_matches_the_golden_fixture_the_native_decoder_reads() {
    let request = golden_request();
    validate_admitted_frame_request(&request).expect("golden request is valid");
    let bytes = encode_admitted_render_command(
        &request,
        &golden_drive(),
        &golden_lease(),
        golden_session(),
    )
    .expect("golden command encodes");
    let path = fixture_dir().join("rust-admitted-render-command-v1.bin");
    if regenerating() {
        std::fs::create_dir_all(fixture_dir()).expect("fixture directory");
        std::fs::write(&path, &bytes).expect("write golden fixture");
        return;
    }
    let committed = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "golden fixture {} is missing ({error}); regenerate with NPC_REGENERATE_WIRE_FIXTURES=1",
            path.display()
        )
    });
    assert!(
        committed == bytes,
        "the Rust encoder no longer produces the committed wire bytes; if the native \
         protocol changed on purpose, regenerate the fixture and update the C++ decode test"
    );
}

/// A hand-built residual response in the exact native layout. Distinct values
/// in every audio clock field make any field-order mistake visible.
fn residual_response_wire(
    schema: u32,
    audio_clock_generation: u64,
    include_clock: bool,
) -> Vec<u8> {
    let lease = golden_lease();
    let mut wire = WireWriter::default();
    wire.u32(PROTOCOL_MAGIC);
    wire.u16(PROTOCOL_VERSION);
    wire.u16(0);
    wire.u64(55);
    wire.u64(CAPTURE_GENERATION);
    // Receipt.
    wire.u32(1);
    for value in [1_u64, 101, 102, 201, 202, 301] {
        wire.u64(value);
    }
    for value in [CAPTURE_GENERATION, 8, 9, 10] {
        wire.u64(value);
    }
    wire.u64(lease.source_frame_sequence);
    wire.u64(lease.source_device_generation);
    wire.u64(lease.source_geometry_epoch);
    wire.i64(900_000_000);
    wire.u8(2);
    wire.u8(0);
    wire.u8(0);
    wire.u8(0);
    wire.u32(10);
    wire.u64(3);
    wire.i64(1_001_000_000);
    wire.i64(1_010_000_000);
    wire.u64(0);
    // Residual proposal.
    wire.boolean(true);
    wire.u32(schema);
    for value in [1_u64, 101, 102, 201, 202, 301] {
        wire.u64(value);
    }
    for value in [CAPTURE_GENERATION, 8, 9, 10] {
        wire.u64(value);
    }
    wire.u64(lease.source_frame_sequence);
    wire.u64(lease.source_device_generation);
    wire.u64(lease.source_geometry_epoch);
    wire.i64(900_000_000);
    wire.u64(lease.source_frame_qpc);
    for value in [0.4_f64, 0.5, 0.1, 0.08] {
        wire.f64(value);
    }
    // Residual texture lease.
    wire.u32(1);
    wire.u32(1);
    wire.u64(501);
    wire.u64(502);
    wire.u32(lease.worker.process_id);
    wire.u32(lease.broker_process_id);
    wire.u64(0x2345);
    wire.u32(lease.adapter_luid as u32);
    wire.i32((lease.adapter_luid >> 32) as i32);
    wire.u64(3);
    wire.u64(4);
    wire.u32(192);
    wire.u32(108);
    wire.u32(768);
    wire.u32(87);
    wire.i64(1_080_000_000);
    wire.f64(0.95);
    if schema >= 2 {
        wire.f64(0.96);
        wire.f64(0.95);
        wire.f64(0.94);
        wire.boolean(false);
        wire.i64(1_000_000_000);
    }
    if include_clock {
        wire.u64(audio_clock_generation);
        wire.u64(SEGMENT_ID);
        wire.u64(960);
        wire.u64(60);
        wire.u64(984);
        wire.u32(48_000);
        wire.u16(1);
        wire.i64(1_000_500_000);
    }
    wire.i64(1_010_000_000);
    wire.u32(0);
    wire.string("residual_proposed_pending_broker_presentation")
        .expect("bounded detail");
    wire.take()
}

fn golden_track() -> VisualTrackBinding {
    golden_lease().track
}

#[test]
fn schema_three_residual_decodes_its_audio_clock_in_native_field_order() {
    let response = decode_worker_response(&residual_response_wire(3, CAPTURE_GENERATION, true))
        .expect("schema 3 response decodes");
    let residual = response.residual.as_ref().expect("residual proposal");
    assert_eq!(residual.schema_version, 3);
    assert_eq!(
        residual.audio_clock,
        Some(WorkerAudioClock {
            stream_generation: CAPTURE_GENERATION,
            segment_id: SEGMENT_ID,
            first_sample_index: 960,
            sample_count: 60,
            playback_sample_index: 984,
            sample_rate: 48_000,
            channels: 1,
            playback_at_ns: 1_000_500_000,
        })
    );
    assert_eq!(residual.produced_at_ns, 1_010_000_000);
    assert_eq!(residual.landmarks_measured_at_ns, 1_000_000_000);
    assert_eq!(
        response.detail,
        "residual_proposed_pending_broker_presentation"
    );
    validate_worker_response(
        &response,
        1,
        &golden_track(),
        &golden_lease(),
        CAPTURE_GENERATION,
    )
    .expect("a schema 3 residual for the expected generation is accepted");
    residual
        .to_broker_proposal(&golden_lease().worker, &golden_lease())
        .expect("the residual maps to a broker proposal");
}

#[test]
fn schema_three_residual_for_another_generation_is_rejected() {
    let response = decode_worker_response(&residual_response_wire(3, CAPTURE_GENERATION + 1, true))
        .expect("wire shape is valid");
    assert!(matches!(
        validate_worker_response(
            &response,
            1,
            &golden_track(),
            &golden_lease(),
            CAPTURE_GENERATION
        ),
        Err(VisualRuntimeError::Malformed)
    ));
}

#[test]
fn schema_two_residual_without_a_clock_still_decodes_and_validates() {
    let response = decode_worker_response(&residual_response_wire(2, 0, false))
        .expect("schema 2 response decodes");
    assert_eq!(response.residual.as_ref().unwrap().audio_clock, None);
    validate_worker_response(
        &response,
        1,
        &golden_track(),
        &golden_lease(),
        CAPTURE_GENERATION,
    )
    .expect("schema 2 stays valid");
}

#[test]
fn residual_schemas_outside_the_native_range_or_with_a_missing_clock_are_rejected() {
    // Schema 3 promises an audio clock; the bytes of a schema 2 body are short.
    assert!(matches!(
        decode_worker_response(&residual_response_wire(3, CAPTURE_GENERATION, false)),
        Err(VisualRuntimeError::Malformed)
    ));
    assert!(matches!(
        decode_worker_response(&residual_response_wire(4, CAPTURE_GENERATION, true)),
        Err(VisualRuntimeError::Malformed)
    ));
    // Schema 1 predates broker occlusion evidence and is never presentable.
    let schema_one = decode_worker_response(&residual_response_wire(1, 0, false))
        .expect("legacy schema decodes");
    assert!(matches!(
        validate_worker_response(
            &schema_one,
            1,
            &golden_track(),
            &golden_lease(),
            CAPTURE_GENERATION
        ),
        Err(VisualRuntimeError::Malformed)
    ));
    // The clock must describe a real span that contains the playback sample.
    let mut response = decode_worker_response(&residual_response_wire(3, CAPTURE_GENERATION, true))
        .expect("schema 3 decodes");
    response
        .residual
        .as_mut()
        .and_then(|residual| residual.audio_clock.as_mut())
        .expect("clock")
        .playback_sample_index = 1_020;
    assert!(matches!(
        validate_worker_response(
            &response,
            1,
            &golden_track(),
            &golden_lease(),
            CAPTURE_GENERATION
        ),
        Err(VisualRuntimeError::Malformed)
    ));
}

/// Values mirror `cpp_schema3_response()` in the C++ service protocol test.
#[test]
fn cpp_encoded_schema_three_response_fixture_decodes_and_validates() {
    let path = fixture_dir().join("cpp-residual-schema3-response-v1.bin");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "native fixture {} is missing ({error}); build and run npc_mouth_service_protocol_tests \
             with NPC_REGENERATE_WIRE_FIXTURES=1",
            path.display()
        )
    });
    let response = decode_worker_response(&bytes).expect("the native encoder's bytes decode");
    assert_eq!(response.status, 0);
    assert_eq!(response.response_to, 55);
    assert_eq!(response.generation, CAPTURE_GENERATION);
    assert_eq!(response.receipt.request_id, 1);
    assert_eq!(response.receipt.disposition, 2, "residual proposed");
    assert_eq!(response.receipt.source_frame_sequence, 11);
    let residual = response.residual.as_ref().expect("residual");
    assert_eq!(residual.schema_version, 3);
    assert_eq!(
        residual.audio_clock,
        Some(WorkerAudioClock {
            stream_generation: CAPTURE_GENERATION,
            segment_id: SEGMENT_ID,
            first_sample_index: 960,
            sample_count: 60,
            playback_sample_index: 984,
            sample_rate: 48_000,
            channels: 1,
            playback_at_ns: 1_000_500_000,
        })
    );
    assert_eq!(residual.landmarks_measured_at_ns, 1_000_000_000);
    assert_eq!(residual.produced_at_ns, 1_010_000_000);
    validate_worker_response(
        &response,
        1,
        &golden_track(),
        &golden_lease(),
        CAPTURE_GENERATION,
    )
    .expect("the native response validates against the leased frame");
    residual
        .to_broker_proposal(&golden_lease().worker, &golden_lease())
        .expect("the native response becomes a broker proposal");
}

// ---------------------------------------------------------------------------
// Receipt ranking and typed diagnostics.

fn failure(request_id: u64, error: VisualRuntimeError) -> VisualPresentationReceipt {
    let mut receipt =
        VisualPresentationReceipt::from_error(request_id, "review_visual_only_bypass", error);
    receipt.cancellation_generation = 7;
    receipt
}

fn inactive_lease_probe(request_id: u64) -> VisualPresentationReceipt {
    failure(
        request_id,
        VisualRuntimeError::BrokerStage {
            stage: "query_visual_audio_envelope",
            source: crate::media_broker::MediaBrokerError::State,
        },
    )
}

#[test]
fn a_later_idle_lease_probe_never_hides_the_real_failure() {
    let real = failure(10, VisualRuntimeError::Worker("residual rejected".into()));
    assert_eq!(real.diagnostic_error_code, Some("visual_worker_rejected"));
    let idle = inactive_lease_probe(11);
    assert_eq!(
        idle.diagnostic_error_code,
        Some("visual_audio_query_failed")
    );
    let inactive = failure(12, VisualRuntimeError::AudioAuthority);
    assert_eq!(
        inactive.diagnostic_error_code,
        Some("visual_audio_envelope_inactive")
    );

    assert!(!better_visual_receipt(&idle, &real));
    assert!(!better_visual_receipt(&inactive, &real));
    assert!(better_visual_receipt(&real, &idle));
    assert!(better_visual_receipt(&real, &inactive));
}

#[test]
fn receipt_ranking_follows_the_furthest_pipeline_stage_reached() {
    let waiting = VisualPresentationReceipt::fail_open_coded(
        1,
        "waiting",
        "visual_waiting_for_presentable_frame",
    );
    let idle = inactive_lease_probe(2);
    let capture = failure(3, VisualRuntimeError::CaptureAuthority);
    let readiness = failure(4, VisualRuntimeError::Admission("pack".into()));
    let stale = failure(5, VisualRuntimeError::StaleLandmarks);
    let worker = failure(6, VisualRuntimeError::Timeout);
    let mut bypass = VisualPresentationReceipt::fail_open(7, "worker_bypass");
    bypass.product_disposition = 3;
    bypass.source_frame_sequence = 11;
    let mut proposed = bypass.clone();
    proposed.request_id = 8;
    proposed.residual_proposed = true;
    let mut presented = proposed.clone();
    presented.request_id = 9;
    presented.presented = true;
    presented.degraded = false;

    let ordered = [
        &waiting, &idle, &capture, &readiness, &stale, &worker, &bypass, &proposed, &presented,
    ];
    for window in ordered.windows(2) {
        assert!(
            better_visual_receipt(window[1], window[0]),
            "{} must outrank {}",
            window[1].detail,
            window[0].detail
        );
        assert!(
            !better_visual_receipt(window[0], window[1]),
            "{} must not outrank {}",
            window[0].detail,
            window[1].detail
        );
    }
}

#[test]
fn the_coordinator_keeps_the_real_failure_after_many_idle_lease_probes() {
    let coordinator = VisualCoordinator::unavailable_for_tests();
    coordinator.record_receipt(failure(1, VisualRuntimeError::Worker("rejected".into())));
    for request_id in 2..40 {
        coordinator.record_receipt(inactive_lease_probe(request_id));
    }
    let best = coordinator
        .best_receipt_for_generation(7)
        .expect("generation receipt");
    assert_eq!(best.request_id, 1);
    assert_eq!(best.diagnostic_error_code, Some("visual_worker_rejected"));
    assert_eq!(
        coordinator.latest_receipt().expect("latest").request_id,
        39,
        "the latest receipt stays chronological"
    );
}

#[test]
fn stream_probe_continues_past_idle_leases_and_stops_at_the_live_stream() {
    assert!(!visual_receipt_ends_stream_probe(&inactive_lease_probe(1)));
    assert!(!visual_receipt_ends_stream_probe(&failure(
        2,
        VisualRuntimeError::AudioAuthority
    )));
    assert!(!visual_receipt_ends_stream_probe(
        &VisualPresentationReceipt::fail_open(3, "generic")
    ));
    for error in [
        VisualRuntimeError::Worker("x".into()),
        VisualRuntimeError::Malformed,
        VisualRuntimeError::Timeout,
        VisualRuntimeError::Admission("x".into()),
        VisualRuntimeError::StaleLandmarks,
    ] {
        assert!(visual_receipt_ends_stream_probe(&failure(4, error)));
    }
}

// ---------------------------------------------------------------------------
// Per-turn provider resolution and the atlas-optional practice path.

#[cfg(debug_assertions)]
mod practice_path {
    use super::*;

    fn resources() -> ReviewTurnResources {
        ReviewTurnResources {
            launch: ReviewOpenSeeFaceLaunch {
                pack_id: "test-pack".into(),
                artifact_root: PathBuf::from("pack-root"),
                detector: verified("detector"),
                landmark: verified("landmark"),
                runtime: verified("runtime"),
                runtime_shared: verified("runtime-shared"),
                content_tree_sha256: "a".repeat(64),
                measurement_sha256: "b".repeat(64),
                exact_target_pid: 4242,
            },
            atlas: None,
        }
    }

    fn verified(name: &str) -> ReviewVerifiedFile {
        ReviewVerifiedFile {
            path: PathBuf::from(name),
            size_bytes: 1,
            sha256: "c".repeat(64),
        }
    }

    #[test]
    fn provider_is_resolved_once_per_turn_and_reused_by_every_frame() {
        let cache = ReviewTurnCache::default();
        cache.begin_turn();
        for _ in 0..40 {
            let resolved = cache
                .get_or_resolve(4242, 9, || Ok(resources()))
                .expect("resolves");
            assert!(resolved.atlas.is_none());
        }
        assert_eq!(cache.resolution_count(), 1, "40 frames, one resolution");
        assert!(cache.lookup(4242, 9).is_some_and(|found| found.is_ok()));

        // A new turn re-verifies the installed pack exactly once.
        cache.begin_turn();
        assert!(cache.lookup(4242, 9).is_none());
        for _ in 0..10 {
            cache
                .get_or_resolve(4242, 9, || Ok(resources()))
                .expect("resolves again");
        }
        assert_eq!(cache.resolution_count(), 2);

        // A different capture generation or process cannot reuse the entry.
        cache
            .get_or_resolve(4242, 10, || Ok(resources()))
            .expect("new generation");
        cache
            .get_or_resolve(777, 10, || Ok(resources()))
            .expect("new process");
        assert_eq!(cache.resolution_count(), 4);
    }

    #[test]
    fn a_failed_resolution_is_remembered_instead_of_rehashing_every_frame() {
        let cache = ReviewTurnCache::default();
        let mut attempts = 0;
        for _ in 0..25 {
            let failed = cache.get_or_resolve(4242, 9, || {
                attempts += 1;
                Err(VisualRuntimeError::Admission(
                    "review OpenSeeFace pack is not installed".into(),
                ))
            });
            match failed {
                Err(VisualRuntimeError::Admission(detail)) => {
                    assert_eq!(detail, "review OpenSeeFace pack is not installed");
                }
                other => panic!("expected the remembered admission failure, got {other:?}"),
            }
        }
        assert_eq!(attempts, 1);
        assert_eq!(cache.resolution_count(), 1);
    }

    fn practice_identity() -> RequestedCharacterMouthAtlasIdentity<'static> {
        RequestedCharacterMouthAtlasIdentity {
            game_profile_id: LEGACY_MARA_GAME_PROFILE_ID,
            character_id: LEGACY_MARA_CHARACTER_ID,
            actor_id: stable_nonzero_id("eclipse-harbor:mara-venn"),
        }
    }

    #[test]
    fn practice_game_without_an_installed_atlas_configures_none_and_is_not_rejected() {
        let atlas = resolve_optional_review_atlas(None, 9, &practice_identity())
            .expect("no atlas root is a valid atlas-free configuration");
        assert!(atlas.is_none());
    }

    #[test]
    fn an_installed_but_invalid_atlas_still_fails_closed() {
        let directory = tempfile::tempdir().expect("scratch directory");
        // The directory exists but holds no atlas manifest.
        let missing_manifest =
            resolve_optional_review_atlas(Some(directory.path()), 9, &practice_identity());
        assert!(matches!(
            missing_manifest,
            Err(VisualRuntimeError::Admission(_))
        ));
        let not_a_directory = directory.path().join("absent");
        assert!(matches!(
            resolve_optional_review_atlas(Some(not_a_directory.as_path()), 9, &practice_identity()),
            Err(VisualRuntimeError::Admission(_))
        ));
    }

    #[test]
    fn turn_resources_without_a_pack_report_admission_and_never_pretend_to_be_ready() {
        // No pack roots at all: the atlas-free practice game still needs the
        // pinned detector/landmark pack, so resolution must fail closed.
        assert!(matches!(
            resolve_review_turn_resources(None, None, None, 4242, 9),
            Err(VisualRuntimeError::Admission(_))
        ));
    }

    /// Presenter whose practice game is selected but unbound, and which never
    /// handles a review frame (the broker holds no practice target).
    struct UnboundPracticePresenter;

    #[async_trait]
    impl AdmittedVisualPresenter for UnboundPracticePresenter {
        async fn present(
            &self,
            request: AdmittedVisualFrameRequest,
            _now_monotonic_millis: u64,
        ) -> VisualPresentationReceipt {
            VisualPresentationReceipt::fail_open(request.request_id, "unexpected_product_path")
        }

        async fn present_project_owned_review(
            &self,
            _request_id: u64,
            _audio: &VisualAudioBinding,
            _sentence_id: u64,
            _now_ns: i64,
        ) -> Option<VisualPresentationReceipt> {
            None
        }

        fn practice_game_awaiting_broker_binding(&self) -> bool {
            true
        }
    }

    struct NoActorLock;

    impl ActorLockSource for NoActorLock {
        fn selected_current(&self, _now_unix_ms: u64) -> Option<Arc<NativeSelectedActorLockV1>> {
            None
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn an_unbound_practice_game_gets_its_own_typed_receipt() {
        let coordinator = VisualCoordinator::from_parts(
            Arc::new(NoActorLock),
            Arc::new(UnboundPracticePresenter),
        );
        coordinator
            .start_audio_bindings(
                7,
                vec![VisualAudioBinding {
                    session_id: "session".into(),
                    turn_id: "turn".into(),
                    generation: 7,
                    stream_id: "stream".into(),
                }],
            )
            .await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        coordinator.stop_turn(Some(7)).await;
        let receipt = coordinator.latest_receipt().expect("receipt");
        assert_eq!(
            receipt.diagnostic_error_code,
            Some("visual_practice_game_not_broker_bound")
        );
        assert_ne!(receipt.diagnostic_error_code, Some("visual_bypassed"));
        assert!(!receipt.presented);
    }

    /// Records which sentence leases are probed and answers like the native
    /// path: idle leases are cheap rejections, one lease is the live stream.
    struct ProbeCountingPresenter {
        probed: Mutex<Vec<String>>,
        live_stream: &'static str,
        live_outcome: fn(u64) -> VisualPresentationReceipt,
    }

    #[async_trait]
    impl AdmittedVisualPresenter for ProbeCountingPresenter {
        async fn present(
            &self,
            request: AdmittedVisualFrameRequest,
            _now_monotonic_millis: u64,
        ) -> VisualPresentationReceipt {
            self.probed
                .lock()
                .unwrap()
                .push(request.audio.stream_id.clone());
            if request.audio.stream_id == self.live_stream {
                (self.live_outcome)(request.request_id)
            } else {
                inactive_lease_probe(request.request_id)
            }
        }

        async fn present_project_owned_review(
            &self,
            _request_id: u64,
            _audio: &VisualAudioBinding,
            _sentence_id: u64,
            _now_ns: i64,
        ) -> Option<VisualPresentationReceipt> {
            None
        }
    }

    /// An always-fresh lock on capture generation 7, so every tick can build a
    /// request inside the 150 ms freshness window.
    #[cfg(windows)]
    struct FreshActorLock;

    #[cfg(windows)]
    impl ActorLockSource for FreshActorLock {
        fn selected_current(&self, _now_unix_ms: u64) -> Option<Arc<NativeSelectedActorLockV1>> {
            Some(Arc::new(super::super::tests::native_actor_lock_fixture(
                7,
                // The coordinator stamps `now` just before asking for the lock,
                // so the frame must be captured slightly in the past.
                monotonic_ns().ok()?.saturating_sub(5_000_000) as u64,
                current_unix_millis() + 10_000,
            )))
        }
    }

    /// Worker failure on the live lease: the next ticks start at that lease and
    /// stop there instead of re-probing every idle sentence of the 16-lease pool.
    #[cfg(windows)]
    #[tokio::test]
    async fn the_live_lease_is_remembered_so_idle_leases_are_not_reprobed_each_tick() {
        let presenter = Arc::new(ProbeCountingPresenter {
            probed: Mutex::new(Vec::new()),
            live_stream: "stream-3",
            live_outcome: |request_id| {
                failure(request_id, VisualRuntimeError::Worker("rejected".into()))
            },
        });
        let coordinator =
            VisualCoordinator::from_parts(Arc::new(FreshActorLock), presenter.clone());
        let streams = (0..6)
            .map(|index| VisualAudioBinding {
                session_id: "session".into(),
                turn_id: "turn".into(),
                generation: 7,
                stream_id: format!("stream-{index}"),
            })
            .collect::<Vec<_>>();
        coordinator.start_audio_bindings(7, streams).await;
        tokio::time::sleep(Duration::from_millis(330)).await;
        coordinator.stop_turn(Some(7)).await;

        let probed = presenter.probed.lock().unwrap().clone();
        assert!(probed.len() >= 6, "several ticks ran: {probed:?}");
        // First tick: streams 0..=3 (three idle, then the live one).
        assert_eq!(
            &probed[..4],
            ["stream-0", "stream-1", "stream-2", "stream-3"]
        );
        // Later ticks go straight to the live stream.
        assert!(
            probed[4..].iter().all(|stream| stream == "stream-3"),
            "idle leases were re-probed: {probed:?}"
        );
        let best = coordinator.best_receipt_for_generation(7).expect("best");
        assert_eq!(best.diagnostic_error_code, Some("visual_worker_rejected"));
    }
}
