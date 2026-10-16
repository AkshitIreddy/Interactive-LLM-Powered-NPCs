#include "npc/mouth_worker/service_protocol.hpp"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#ifndef NPC_MOUTH_WIRE_FIXTURE_DIR
#error "NPC_MOUTH_WIRE_FIXTURE_DIR must name native/mouth-worker/protocol/fixtures"
#endif

namespace {

using namespace npc::mouth;
using namespace npc::mouth::service;

int failures = 0;

void expect(const bool condition, const std::string_view message) {
    if (!condition) {
        std::cerr << "FAIL: " << message << '\n';
        ++failures;
    }
}

SessionBindingV1 session() {
    SessionBindingV1 value{};
    for (std::size_t index = 0; index < value.nonce.size(); ++index) {
        value.nonce[index] = static_cast<std::byte>(index + 1U);
    }
    value.session_id_high = 101U;
    value.session_id_low = 102U;
    return value;
}

RenderCurrentFrameCommandV1 render_command() {
    RenderCurrentFrameCommandV1 value{};
    value.request = {1U, 101U, 102U, 201U, 202U, 301U};
    value.source.schema_version = 1U;
    value.source.session = session();
    value.source.texture.schema_version = 1U;
    value.source.texture.transport = LeaseTransport::d3d11_shared_nt_handle;
    value.source.texture.lease_nonce_high = 401U;
    value.source.texture.lease_nonce_low = 402U;
    value.source.texture.owner_process_id = 50U;
    value.source.texture.intended_consumer_process_id = 60U;
    value.source.texture.native_handle_value = 0x1234U;
    value.source.texture.adapter_luid_low = 77U;
    value.source.texture.keyed_mutex_acquire_key = 1U;
    value.source.texture.keyed_mutex_release_key = 2U;
    value.source.texture.width = 1920U;
    value.source.texture.height = 1080U;
    value.source.texture.stride_bytes = 1920U * 4U;
    value.source.texture.expires_at_ns = 1'080'000'000;
    value.source.broker_process_id = 50U;
    value.source.broker_process_creation_time = 600U;
    value.source.broker_executable_name = "npc-media-broker.exe";
    value.source.source_frame_qpc = 900U;
    value.source.qpc_frequency = 10'000'000U;
    value.track = {7U, 8U, 9U, 10U};
    value.frame = {11U, 12U, 13U, 1'000'000'000};
    value.landmarks.schema_version = 1U;
    value.landmarks.provider_instance_id = 14U;
    value.landmarks.track = value.track;
    value.landmarks.frame = value.frame;
    value.landmarks.source_frame_qpc = value.source.source_frame_qpc;
    value.landmarks.qpc_frequency = value.source.qpc_frequency;
    value.landmarks.face_bounds = {0.2, 0.1, 0.4, 0.7};
    for (std::size_t index = 0; index < value.landmarks.landmarks.size(); ++index) {
        value.landmarks.landmarks[index] = {
            0.3 + static_cast<double>(index % 8U) * 0.01,
            0.4 + static_cast<double>(index / 8U) * 0.01,
            0.95,
        };
    }
    value.landmarks.pose = {1.0, 2.0, 3.0};
    value.landmarks.detector_confidence = 0.96;
    value.landmarks.landmark_confidence = 0.95;
    value.landmarks.visibility_ratio = 0.94;
    value.landmarks.measured_at_ns = 1'000'000'000;
    value.appearance = {1U, 8U, 15U, 16U, 17U, 18U, 19U,
                        0.93, 0.87, 0.04, true, true, false};
    value.resources = {1U, VisualPressure::elevated_cpu, 10U, true};
    value.drive.kind = DriveKind::pcm_window;
    value.drive.clock = {7U, 301U, 200U, 4U, 202U, 24'000U, 1U, 1'000'000'000};
    value.drive.interleaved_pcm = {0.1F, -0.2F, 0.3F, -0.4F};
    value.deadline_ns = 1'070'000'000;
    return value;
}

RenderWithAdmittedLandmarksCommandV1 admitted_render_command() {
    const auto supplied = render_command();
    RenderWithAdmittedLandmarksCommandV1 value{};
    value.request = supplied.request;
    value.source = supplied.source;
    value.track = supplied.track;
    value.frame = supplied.frame;
    value.seed_face_bounds = supplied.landmarks.face_bounds;
    value.appearance = supplied.appearance;
    value.resources = supplied.resources;
    value.drive = supplied.drive;
    value.deadline_ns = supplied.deadline_ns;
    value.sealed_click_spatial_authority = true;
    return value;
}

InstallCharacterMouthAtlasCommandV1 atlas_command() {
    InstallCharacterMouthAtlasCommandV1 value{};
    value.atlas.cancellation_generation = 7U;
    value.atlas.actor_id = 8U;
    value.atlas.identity_revision = 91U;
    for (std::uint32_t index = 0U; index < 4U; ++index) {
        MouthAtlasState state{};
        state.coefficients.jaw_open = static_cast<double>(index) / 3.0;
        state.coefficients.lip_close = 1.0 - state.coefficients.jaw_open;
        state.appearance.width = 24U;
        state.appearance.height = 16U;
        state.appearance.stride_bytes = 96U;
        state.appearance.enrolled_pose = {
            static_cast<double>(index), -static_cast<double>(index), 0.5,
        };
        state.appearance.premultiplied_bgra.assign(96U * 16U, 0U);
        for (std::size_t offset = 0U;
             offset < state.appearance.premultiplied_bgra.size(); offset += 4U) {
            state.appearance.premultiplied_bgra[offset] =
                static_cast<std::uint8_t>(20U + index);
            state.appearance.premultiplied_bgra[offset + 1U] = 40U;
            state.appearance.premultiplied_bgra[offset + 2U] = 80U;
            state.appearance.premultiplied_bgra[offset + 3U] = 255U;
        }
        value.atlas.states.push_back(std::move(state));
    }
    return value;
}

void test_render_and_response_round_trip() {
    const auto source = render_command();
    const auto encoded = encode_render_command(source);
    expect(encoded.has_value(), "bounded render command encodes");
    const auto decoded = decode_render_command(*encoded);
    expect(decoded.has_value(), "render command decodes");
    expect(decoded->request == source.request, "request identity round-trips exactly");
    expect(decoded->track == source.track && decoded->frame == source.frame,
           "actor track and exact frame round-trip");
    expect(decoded->source.texture.native_handle_value == 0x1234U &&
               decoded->source.broker_executable_name == "npc-media-broker.exe",
           "source handle and broker identity round-trip");
    expect(decoded->landmarks.landmarks[54U].confidence == 0.95,
           "all 66 typed landmark confidences survive the wire");
    expect(decoded->drive.kind == DriveKind::pcm_window &&
               decoded->drive.interleaved_pcm == source.drive.interleaved_pcm,
           "causal PCM drive survives the wire without quantization");

    WorkerResponseV1 response{};
    response.response_to_sequence = 55U;
    response.cancellation_generation = 7U;
    response.receipt.request = source.request;
    response.receipt.track = source.track;
    response.receipt.source_frame = source.frame;
    response.receipt.disposition = PresentationDisposition::residual_proposed;
    response.receipt.signal_disposition = SignalDisposition::accepted;
    response.receipt.worker_disposition = Disposition::residual_ready;
    response.receipt.drive_kind = DriveKind::pcm_window;
    response.receipt.completed_at_ns = 1'010'000'000;
    ResidualProposalV1 proposal{};
    proposal.schema_version = 3U;
    proposal.request = source.request;
    proposal.track = source.track;
    proposal.source_frame = source.frame;
    proposal.source_frame_qpc = source.source.source_frame_qpc;
    proposal.normalized_bounds = {0.4, 0.5, 0.1, 0.08};
    proposal.residual = source.source.texture;
    proposal.residual.owner_process_id = 60U;
    proposal.residual.intended_consumer_process_id = 50U;
    proposal.confidence = 0.95;
    proposal.audio_clock = source.drive.clock;
    proposal.produced_at_ns = 1'010'000'000;
    response.residual = proposal;
    response.detail = "residual proposed; broker presentation remains authoritative";
    const auto encoded_response = encode_response(response);
    const auto decoded_response = decode_response(*encoded_response);
    expect(decoded_response && decoded_response->residual &&
               decoded_response->residual->source_frame == source.frame &&
               decoded_response->residual->audio_clock.playback_sample_index == 202U &&
               decoded_response->residual->audio_clock.sample_count == 4U,
           "receipt and exact audio-bound residual proposal round-trip");
    expect(decoded_response->detail == response.detail,
           "honest presentation detail round-trips");

    auto unknown_response_schema = response;
    unknown_response_schema.residual->schema_version = 4U;
    expect(!encode_response(unknown_response_schema),
           "unknown residual proposal schema fails closed before transport");
}

void test_authenticated_envelope_and_framing() {
    EnvelopeV1 envelope{};
    envelope.session = session();
    envelope.command = CommandKind::render_current_frame;
    envelope.sequence = 1U;
    envelope.cancellation_generation = 7U;
    envelope.deadline_ns = 1'050'000'000;
    envelope.payload = *encode_render_command(render_command());
    const auto encoded = encode_envelope(envelope);
    const auto decoded = decode_envelope(*encoded);
    expect(decoded && decoded->sequence == 1U && decoded->payload == envelope.payload,
           "authenticated envelope round-trips");
    const auto framed = frame_message(*encoded);
    std::array<std::byte, 4> prefix{};
    std::copy_n(framed.begin(), 4U, prefix.begin());
    expect(decode_frame_size(prefix) == encoded->size(), "length prefix is exact");

    EnvelopeValidator validator({session(), 5'000'000'000});
    expect(validator.validate(*decoded, 1'000'000'000, 7U) == StatusCode::ok,
           "fresh authenticated command is accepted");
    expect(validator.validate(*decoded, 1'000'000'000, 7U) ==
               StatusCode::sequence_replayed,
           "replayed sequence is rejected");

    auto wrong_session = envelope;
    wrong_session.sequence = 2U;
    ++wrong_session.session.session_id_low;
    expect(validator.validate(wrong_session, 1'000'000'000, 7U) ==
               StatusCode::session_mismatch,
           "session mismatch fails closed");
    auto wrong_nonce = envelope;
    wrong_nonce.sequence = 2U;
    wrong_nonce.session.nonce[0] = std::byte{0xff};
    expect(validator.validate(wrong_nonce, 1'000'000'000, 7U) ==
               StatusCode::authentication_failed,
           "nonce mismatch fails closed");
    auto wrong_generation = envelope;
    wrong_generation.sequence = 2U;
    wrong_generation.cancellation_generation = 6U;
    expect(validator.validate(wrong_generation, 1'000'000'000, 7U) ==
               StatusCode::cancellation_mismatch,
           "stale cancellation generation is rejected");
}

void test_admitted_provider_command_round_trip() {
    const auto source = admitted_render_command();
    const auto encoded = encode_admitted_render_command(source);
    expect(encoded.has_value(), "bounded admitted-provider command encodes");
    const auto decoded = decode_admitted_render_command(*encoded);
    expect(decoded.has_value(), "admitted-provider command decodes");
    expect(decoded && decoded->request == source.request && decoded->track == source.track &&
               decoded->frame == source.frame,
           "provider command preserves request, actor track, and exact frame");
    expect(decoded && decoded->source.source_frame_qpc == source.source.source_frame_qpc &&
               decoded->source.qpc_frequency == source.source.qpc_frequency,
           "provider command preserves exact source QPC binding");
    expect(decoded && decoded->seed_face_bounds.x == source.seed_face_bounds.x &&
                decoded->seed_face_bounds.width == source.seed_face_bounds.width,
           "provider command carries only the identity-authoritative seed face ROI");
    expect(decoded && decoded->sealed_click_spatial_authority,
           "provider command preserves explicit sealed-click spatial authority");

    auto truncated = *encoded;
    truncated.pop_back();
    expect(!decode_admitted_render_command(truncated),
           "truncated admitted-provider command fails closed");
}

void test_actor_candidate_discovery_round_trip() {
    const auto admitted = admitted_render_command();
    DiscoverActorCandidatesCommandV1 source{};
    source.source = admitted.source;
    source.discovery_track = admitted.track;
    source.frame = admitted.frame;
    source.deadline_ns = admitted.deadline_ns;
    const auto encoded = encode_actor_candidate_discovery(source);
    const auto decoded = encoded ? decode_actor_candidate_discovery(*encoded) : std::nullopt;
    expect(decoded && decoded->source.source_frame_qpc == source.source.source_frame_qpc &&
               decoded->discovery_track == source.discovery_track &&
               decoded->frame == source.frame && decoded->deadline_ns == source.deadline_ns,
           "candidate discovery stays bound to the exact native source frame and track");

    WorkerResponseV1 response{};
    response.response_to_sequence = 77U;
    response.cancellation_generation = 10U;
    response.receipt.source_frame = source.frame;
    response.actor_candidates.push_back({
        source.discovery_track.actor_id,
        source.discovery_track.track_id,
        source.discovery_track.track_epoch,
        {0.2, 0.1, 0.4, 0.7},
        0.96,
    });
    response.detail = "admitted_actor_candidate_ready";
    const auto response_wire = encode_response(response);
    const auto response_round_trip = response_wire ? decode_response(*response_wire) : std::nullopt;
    expect(response_round_trip && response_round_trip->actor_candidates.size() == 1U &&
               response_round_trip->receipt.source_frame.sequence == source.frame.sequence &&
               response_round_trip->actor_candidates.front().track_id ==
                   source.discovery_track.track_id,
           "detected candidate response preserves exact source-frame and native track binding");

    response.actor_candidates.front().bounds.width = 0.0;
    expect(!encode_response(response), "invalid detected candidate geometry fails closed");
}

void test_provider_configuration_round_trip() {
    auto launch = AdmittedLandmarkProviderLaunchV1{};
    const auto root = std::filesystem::absolute("fixture-openseeface-pack");
    launch.pack_id = std::string(admitted_openseeface_pack_id_v1);
    launch.pack_revision = std::string(admitted_openseeface_revision_v1);
    launch.artifact_root = root;
    launch.detector_model = root / "models" / "mnv3_detection_opt.onnx";
    launch.landmark_model = root / "models" / "lm_model1_opt.onnx";
    launch.runtime_library = root / "runtime" / "onnxruntime.dll";
    launch.runtime_shared_library = root / "runtime" / "onnxruntime_providers_shared.dll";
    launch.detector_size_bytes = 568'302U;
    launch.landmark_size_bytes = 4'842'329U;
    launch.runtime_size_bytes = 14'854'688U;
    launch.runtime_shared_size_bytes = 19'456U;
    launch.detector_sha256.assign(64U, 'a');
    launch.landmark_sha256.assign(64U, 'b');
    launch.runtime_sha256.assign(64U, 'c');
    launch.runtime_shared_sha256.assign(64U, 'e');
    launch.measured_envelope_sha256.assign(64U, 'd');
    launch.runtime_revision = std::string(admitted_openseeface_runtime_revision_v1);
    launch.backend = std::string(admitted_openseeface_backend_v1);
    launch.maximum_signal_rate_hz = 15U;
    launch.inference_threads = 1U;
    launch.exact_target_process_id = 42U;
    const auto encoded = encode_provider_configuration({launch});
    const auto decoded = encoded ? decode_provider_configuration(*encoded) : std::nullopt;
    expect(decoded && decoded->launch.detector_model == launch.detector_model &&
               decoded->launch.runtime_library == launch.runtime_library &&
               decoded->launch.detector_size_bytes == launch.detector_size_bytes &&
               decoded->launch.measured_envelope_sha256 == launch.measured_envelope_sha256,
           "exact provider paths, sizes, hashes, envelope, and target survive authenticated wire");

    auto setup = launch;
    setup.schema_version = 2U;
    setup.exact_target_process_id = 0U;
    setup.provider_load_self_test = true;
    const auto encoded_setup = encode_provider_configuration({setup});
    const auto decoded_setup = encoded_setup
        ? decode_provider_configuration(*encoded_setup)
        : std::nullopt;
    expect(decoded_setup && decoded_setup->launch.provider_load_self_test &&
               decoded_setup->launch.exact_target_process_id == 0U,
           "setup provider-load purpose round-trips without target authority");

    setup.exact_target_process_id = 42U;
    expect(!encode_provider_configuration({setup}),
           "setup provider-load purpose rejects target authority");
    setup.provider_load_self_test = false;
    setup.exact_target_process_id = 0U;
    expect(!encode_provider_configuration({setup}),
           "runtime provider purpose still requires exact target authority");
}

void test_character_mouth_atlas_round_trip() {
    const auto source = atlas_command();
    const auto encoded = encode_character_mouth_atlas(source);
    const auto decoded = encoded ? decode_character_mouth_atlas(*encoded) : std::nullopt;
    expect(decoded && decoded->atlas.actor_id == source.atlas.actor_id &&
               decoded->atlas.identity_revision == source.atlas.identity_revision &&
               decoded->atlas.states.size() == source.atlas.states.size(),
           "identity atlas ownership and state count survive the authenticated wire");
    expect(decoded &&
               decoded->atlas.states[3U].appearance.enrolled_pose.yaw == 3.0 &&
               decoded->atlas.states[3U].coefficients.jaw_open == 1.0 &&
               decoded->atlas.states[3U].appearance.premultiplied_bgra ==
                   source.atlas.states[3U].appearance.premultiplied_bgra,
           "atlas coefficients, pose, and premultiplied pixels round-trip byte-exactly");

    auto truncated = *encoded;
    truncated.pop_back();
    expect(!decode_character_mouth_atlas(truncated),
           "truncated identity atlas fails closed");
    auto too_few_states = source;
    too_few_states.atlas.states.resize(3U);
    expect(!encode_character_mouth_atlas(too_few_states),
           "underspecified identity atlas is rejected before transport");

    auto oral = source;
    oral.atlas.schema_version = 2U;
    expect(!encode_character_mouth_atlas(oral),
           "schema two cannot silently reinterpret legacy full-lip pixels");
    for (auto& state : oral.atlas.states) {
        state.appearance.representation = MouthPatchRepresentation::normalized_oral_interior_v1;
    }
    const auto oral_encoded = encode_character_mouth_atlas(oral);
    const auto oral_decoded = oral_encoded ? decode_character_mouth_atlas(*oral_encoded) : std::nullopt;
    expect(oral_decoded && oral_decoded->atlas.schema_version == 2U &&
               oral_decoded->atlas.states[0].appearance.representation ==
                   MouthPatchRepresentation::normalized_oral_interior_v1 &&
               oral_decoded->atlas.states[0].appearance.premultiplied_bgra ==
                   oral.atlas.states[0].appearance.premultiplied_bgra,
           "explicit oral representation round-trips without changing pixels");
    oral.atlas.schema_version = 1U;
    expect(!encode_character_mouth_atlas(oral),
           "oral data cannot be serialized as a legacy full-lip atlas");

    auto strip = source;
    strip.atlas.schema_version = 4U;
    for (auto& state : strip.atlas.states) {
        state.appearance.representation = MouthPatchRepresentation::normalized_oral_strip_v1;
        state.appearance.reference_context_mean = 73.125;
        state.appearance.refine_source_edges = true;
    }
    const auto strip_encoded = encode_character_mouth_atlas(strip);
    const auto strip_decoded = strip_encoded
        ? decode_character_mouth_atlas(*strip_encoded) : std::nullopt;
    expect(strip_decoded && strip_decoded->atlas.schema_version == 4U &&
               strip_decoded->atlas.states[2U].appearance.reference_context_mean == 73.125 &&
               strip_decoded->atlas.states[2U].appearance.refine_source_edges &&
               strip_decoded->atlas.states[2U].appearance.premultiplied_bgra ==
                   strip.atlas.states[2U].appearance.premultiplied_bgra,
           "oral strips preserve exposure, geometry policy and exact pixels on the wire");
    if (strip_encoded) {
        auto invalid_edge_policy = *strip_encoded;
        invalid_edge_policy[140U] = std::byte{2U};
        expect(!decode_character_mouth_atlas(invalid_edge_policy),
               "schema four rejects non-boolean geometry policy on the wire");
        auto truncated_strip = *strip_encoded;
        truncated_strip.pop_back();
        expect(!decode_character_mouth_atlas(truncated_strip), "truncated schema four fails closed");
    }
    strip.atlas.states.front().appearance.reference_context_mean = 0.0;
    expect(!encode_character_mouth_atlas(strip), "zero reference exposure is rejected");

    auto photometric = source;
    photometric.atlas.schema_version = 3U;
    expect(!encode_character_mouth_atlas(photometric),
           "schema three cannot silently reinterpret legacy full-lip pixels");
    for (auto& state : photometric.atlas.states) {
        state.appearance.representation =
            MouthPatchRepresentation::photometric_full_lip_reference_v1;
    }
    const auto photometric_encoded = encode_character_mouth_atlas(photometric);
    const auto photometric_decoded = photometric_encoded
        ? decode_character_mouth_atlas(*photometric_encoded)
        : std::nullopt;
    expect(photometric_decoded && photometric_decoded->atlas.schema_version == 3U &&
               photometric_decoded->atlas.states[0U].appearance.representation ==
                   MouthPatchRepresentation::photometric_full_lip_reference_v1 &&
               photometric_decoded->atlas.states[3U].appearance.premultiplied_bgra ==
                   photometric.atlas.states[3U].appearance.premultiplied_bgra,
           "photometric reference representation round-trips over the unchanged atlas wire");
    photometric.atlas.schema_version = 2U;
    expect(!encode_character_mouth_atlas(photometric),
           "photometric reference pixels cannot be serialized as normalized oral data");
    auto unknown_schema = *encoded;
    unknown_schema[0] = std::byte{99};
    expect(!decode_character_mouth_atlas(unknown_schema), "unknown atlas representation fails closed");
}

void test_malformed_and_bounded_payloads() {
    auto encoded = *encode_render_command(render_command());
    encoded.pop_back();
    expect(!decode_render_command(encoded), "truncated render command is rejected");
    std::vector<std::byte> oversized(maximum_message_bytes + 1U);
    expect(!decode_envelope(oversized), "oversized envelope is rejected before allocation");
    std::array<std::byte, 4> zero{};
    expect(!decode_frame_size(zero), "zero-length frame is rejected");

    auto large_pcm = render_command();
    large_pcm.drive.interleaved_pcm.assign(maximum_pcm_samples + 1U, 0.0F);
    expect(!encode_render_command(large_pcm), "oversized PCM window is rejected");

    const CancelGenerationCommandV1 cancel{9U};
    expect(decode_cancel_command(encode_cancel_command(cancel))->new_generation == 9U,
           "cancel command round-trips");
    const AcknowledgeResidualCommandV1 ack{77U, 88U, true};
    const auto decoded_ack = decode_acknowledgement(encode_acknowledgement(ack));
    expect(decoded_ack && decoded_ack->lease_nonce_high == 77U && decoded_ack->presented,
           "residual acknowledgement round-trips");
}


// ---------------------------------------------------------------------------
// Cross-language wire fixtures. The Rust control plane and this decoder were
// once changed independently, which made every practice-game frame fail decode.
// Two committed golden files now pin both directions:
//   rust-admitted-render-command-v1.bin   written by the Rust encoder
//                                         (visual_runtime_wire_contract_tests.rs)
//   cpp-residual-schema3-response-v1.bin  written by this encoder
// Regenerate the file a side owns with NPC_REGENERATE_WIRE_FIXTURES=1.

#if defined(_MSC_VER)
#pragma warning(push)
#pragma warning(disable : 4996)
#endif
bool regenerating_wire_fixtures() {
    return std::getenv("NPC_REGENERATE_WIRE_FIXTURES") != nullptr;
}
#if defined(_MSC_VER)
#pragma warning(pop)
#endif

std::filesystem::path wire_fixture_path(const std::string_view name) {
    return std::filesystem::path(NPC_MOUTH_WIRE_FIXTURE_DIR) / std::string(name);
}

std::optional<std::vector<std::byte>> read_wire_fixture(const std::string_view name) {
    std::ifstream stream(wire_fixture_path(name), std::ios::binary);
    if (!stream) return std::nullopt;
    const std::vector<char> raw((std::istreambuf_iterator<char>(stream)),
                                std::istreambuf_iterator<char>());
    std::vector<std::byte> bytes(raw.size());
    std::transform(raw.begin(), raw.end(), bytes.begin(),
                   [](const char value) { return static_cast<std::byte>(value); });
    return bytes;
}

bool write_wire_fixture(const std::string_view name, const std::vector<std::byte>& bytes) {
    std::filesystem::create_directories(std::filesystem::path(NPC_MOUTH_WIRE_FIXTURE_DIR));
    std::ofstream stream(wire_fixture_path(name), std::ios::binary | std::ios::trunc);
    if (!stream) return false;
    for (const auto byte : bytes) stream.put(static_cast<char>(std::to_integer<unsigned char>(byte)));
    return static_cast<bool>(stream);
}

// Decodes the exact bytes the Rust encoder produces for its golden request. The
// values below mirror golden_request()/golden_lease()/golden_drive() in
// apps/control/src-tauri/src/visual_runtime_wire_contract_tests.rs.
void test_rust_admitted_render_fixture_decodes() {
    const auto fixture = read_wire_fixture("rust-admitted-render-command-v1.bin");
    expect(fixture.has_value(), "Rust golden render command fixture is committed");
    if (!fixture) return;
    const auto decoded = decode_admitted_render_command(*fixture);
    expect(decoded.has_value(),
           "the Rust encoder's command decodes (drive carries sample_count and playback_sample_index)");
    if (!decoded) return;

    expect(decoded->request == ProductRequestIdentity{1U, 101U, 102U, 201U, 202U, 301U},
           "request identity survives the Rust encoding");
    expect(decoded->track == TrackBinding{42U, 8U, 9U, 10U},
           "track carries the broker capture generation first");
    expect(decoded->frame == FrameIdentity{11U, 12U, 13U, 900'000'000},
           "exact frame identity survives the Rust encoding");
    expect(decoded->source.texture.native_handle_value == 0x1234U &&
               decoded->source.texture.owner_process_id == 50U &&
               decoded->source.texture.intended_consumer_process_id == 60U &&
               decoded->source.texture.adapter_luid_low == 77U &&
               decoded->source.texture.adapter_luid_high == 2 &&
               decoded->source.texture.width == 1'920U &&
               decoded->source.texture.height == 1'080U &&
               decoded->source.texture.stride_bytes == 7'680U &&
               decoded->source.texture.format == PixelFormat::bgra8_unorm_premultiplied &&
               decoded->source.texture.expires_at_ns == 1'080'000'000,
           "texture lease survives the Rust encoding");
    expect(decoded->source.broker_process_id == 50U &&
               decoded->source.broker_executable_name == "npc-media-broker.exe" &&
               decoded->source.source_frame_qpc == 9'000'000U &&
               decoded->source.qpc_frequency == 10'000'000U &&
               decoded->source.session.session_id_high == 101U &&
               decoded->source.session.session_id_low == 102U &&
               decoded->source.session.nonce[0] == std::byte{1} &&
               decoded->source.session.nonce[31] == std::byte{32},
           "broker identity, QPC clock and session survive the Rust encoding");
    expect(decoded->seed_face_bounds.x == 0.2 && decoded->seed_face_bounds.height == 0.7,
           "seed face bounds survive the Rust encoding");
    expect(decoded->appearance.runtime_actor_id == 8U &&
               decoded->appearance.descriptor_revision == 3U &&
               decoded->appearance.expected_descriptor_digest_low == 22U &&
               decoded->appearance.observed_descriptor_digest_high == 23U &&
               decoded->appearance.similarity == 0.93 && decoded->appearance.identity_locked &&
               decoded->appearance.target_visible && !decoded->appearance.scene_transition,
           "appearance evidence survives the Rust encoding");
    expect(decoded->resources.pressure == VisualPressure::elevated_cpu &&
               decoded->resources.admitted_signal_rate_hz == 10U &&
               decoded->resources.local_visuals_admitted,
           "resource state survives the Rust encoding");

    const auto& clock = decoded->drive.clock;
    expect(decoded->drive.kind == DriveKind::explicit_coefficients,
           "envelope coefficient drive kind survives the Rust encoding");
    expect(clock.stream_generation == 42U && clock.segment_id == 0x1122'3344'5566'7788ULL &&
               clock.first_sample_index == 960U && clock.sample_count == 60U &&
               clock.playback_sample_index == 984U && clock.sample_rate == 48'000U &&
               clock.channels == 1U && clock.playback_at_ns == 1'000'500'000,
           "the whole audio clock, including sample_count and playback_sample_index, "
           "survives the Rust encoding");
    expect(clock.playback_sample_index >= clock.first_sample_index &&
               clock.playback_sample_index < clock.first_sample_index + clock.sample_count,
           "the Rust drive's playback sample lies inside its declared span");
    expect(decoded->drive.coefficients.jaw_open == 0.125 &&
               decoded->drive.coefficients.lip_close == 0.25 &&
               decoded->drive.coefficients.lower_lip_depress == 1.0 &&
               decoded->drive.interleaved_pcm.empty(),
           "coefficient channels survive the Rust encoding without PCM");
    expect(!decoded->sealed_click_spatial_authority && decoded->deadline_ns == 1'070'000'000,
           "authority flag and deadline survive the Rust encoding");

    const auto reencoded = encode_admitted_render_command(*decoded);
    expect(reencoded.has_value() && *reencoded == *fixture,
           "decoding then re-encoding the Rust bytes is the identity (same field order)");
}

ResidualProposalV1 schema3_fixture_proposal(const RenderWithAdmittedLandmarksCommandV1& command) {
    ResidualProposalV1 proposal{};
    proposal.schema_version = 3U;
    proposal.request = command.request;
    proposal.track = command.track;
    proposal.source_frame = command.frame;
    proposal.source_frame_qpc = command.source.source_frame_qpc;
    proposal.normalized_bounds = {0.4, 0.5, 0.1, 0.08};
    proposal.residual.schema_version = 1U;
    proposal.residual.transport = LeaseTransport::d3d11_shared_nt_handle;
    proposal.residual.lease_nonce_high = 501U;
    proposal.residual.lease_nonce_low = 502U;
    proposal.residual.owner_process_id = 60U;
    proposal.residual.intended_consumer_process_id = 50U;
    proposal.residual.native_handle_value = 0x2345U;
    proposal.residual.adapter_luid_low = 77U;
    proposal.residual.adapter_luid_high = 2;
    proposal.residual.keyed_mutex_acquire_key = 3U;
    proposal.residual.keyed_mutex_release_key = 4U;
    proposal.residual.width = 192U;
    proposal.residual.height = 108U;
    proposal.residual.stride_bytes = 768U;
    proposal.residual.format = PixelFormat::bgra8_unorm_premultiplied;
    proposal.residual.expires_at_ns = 1'080'000'000;
    proposal.confidence = 0.95;
    proposal.detector_confidence = 0.96;
    proposal.landmark_confidence = 0.95;
    proposal.visibility_ratio = 0.94;
    proposal.mouth_occluded = false;
    proposal.landmarks_measured_at_ns = 1'000'000'000;
    proposal.audio_clock = command.drive.clock;
    proposal.produced_at_ns = 1'010'000'000;
    return proposal;
}

// The values here are asserted by
// cpp_encoded_schema_three_response_fixture_decodes_and_validates in
// visual_runtime_wire_contract_tests.rs; keep the two in sync.
void test_cpp_schema3_response_fixture() {
    const auto rust_fixture = read_wire_fixture("rust-admitted-render-command-v1.bin");
    expect(rust_fixture.has_value(), "the Rust command fixture seeds the response fixture");
    if (!rust_fixture) return;
    const auto command = decode_admitted_render_command(*rust_fixture);
    expect(command.has_value(), "the Rust command fixture decodes");
    if (!command) return;

    WorkerResponseV1 response{};
    response.response_to_sequence = 55U;
    response.cancellation_generation = 42U;
    response.receipt.request = command->request;
    response.receipt.track = command->track;
    response.receipt.source_frame = command->frame;
    response.receipt.disposition = PresentationDisposition::residual_proposed;
    response.receipt.signal_disposition = SignalDisposition::accepted;
    response.receipt.worker_disposition = Disposition::residual_ready;
    response.receipt.drive_kind = DriveKind::explicit_coefficients;
    response.receipt.admitted_signal_rate_hz = 10U;
    response.receipt.latch_generation = 3U;
    response.receipt.submitted_at_ns = 1'001'000'000;
    response.receipt.completed_at_ns = 1'010'000'000;
    response.residual = schema3_fixture_proposal(*command);
    response.detail = "residual_proposed_pending_broker_presentation";

    const auto encoded = encode_response(response);
    expect(encoded.has_value(), "the schema 3 residual response encodes");
    if (!encoded) return;
    const auto decoded = decode_response(*encoded);
    expect(decoded && decoded->residual &&
               decoded->residual->audio_clock.stream_generation == 42U &&
               decoded->residual->audio_clock.sample_count == 60U &&
               decoded->residual->audio_clock.playback_sample_index == 984U &&
               decoded->residual->audio_clock.playback_at_ns == 1'000'500'000,
           "the schema 3 audio clock round-trips through the native codec");

    if (regenerating_wire_fixtures()) {
        expect(write_wire_fixture("cpp-residual-schema3-response-v1.bin", *encoded),
               "native golden response fixture is written");
        return;
    }
    const auto committed = read_wire_fixture("cpp-residual-schema3-response-v1.bin");
    expect(committed.has_value(), "native golden response fixture is committed");
    expect(committed.has_value() && *committed == *encoded,
           "the native encoder still produces the committed schema 3 response bytes the "
           "Rust decoder is tested against (regenerate with NPC_REGENERATE_WIRE_FIXTURES=1 "
           "and rerun the Rust wire contract tests if this change is intentional)");
}

} // namespace

int main() {
    test_render_and_response_round_trip();
    test_authenticated_envelope_and_framing();
    test_admitted_provider_command_round_trip();
    test_actor_candidate_discovery_round_trip();
    test_provider_configuration_round_trip();
    test_character_mouth_atlas_round_trip();
    test_malformed_and_bounded_payloads();
    test_rust_admitted_render_fixture_decodes();
    test_cpp_schema3_response_fixture();
    if (failures != 0) {
        std::cerr << failures << " service protocol assertion(s) failed\n";
        return 1;
    }
    std::cout << "PASS: authenticated bounded mouth-worker service protocol\n";
    return 0;
}
