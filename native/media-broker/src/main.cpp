#include "npc/media_broker/broker.hpp"
#include "npc/media_broker/ipc.hpp"
#include "npc/media_broker/service_config.hpp"
#include "npc/media_broker/target_policy.hpp"

#ifdef _WIN32
#include "windows_service.hpp"
#include "windows_input.hpp"
#include "windows_presentation_context.hpp"
#include "windows_playback.hpp"
#include <Windows.h>
#endif

#include <chrono>
#include <cstddef>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <span>
#include <string_view>
#include <thread>
#include <type_traits>
#include <vector>

namespace {

using namespace npc::media;

template <typename T>
void append_little(std::vector<std::byte>& output, T value) {
    static_assert(std::is_unsigned_v<T>);
    for (unsigned shift = 0; shift < sizeof(T) * 8; shift += 8) {
        output.push_back(static_cast<std::byte>((value >> shift) & 0xffU));
    }
}

[[nodiscard]] std::vector<std::byte> diagnostics_payload(const Diagnostics& value) {
    std::vector<std::byte> result;
    result.reserve(96);
    append_little(result, static_cast<std::uint32_t>(value.state));
    append_little(result, static_cast<std::uint32_t>(value.capture_backend));
    append_little(result, static_cast<std::uint32_t>(value.overlay_backend));
    append_little(result, static_cast<std::uint32_t>(value.capture_audio));
    append_little(result, static_cast<std::uint32_t>(value.render_audio));
    append_little(result, static_cast<std::uint32_t>(value.target_state));
    append_little(result, value.device_generation);
    append_little(result, value.audio_device_generation);
    append_little(result, value.cancellation_generation);
    append_little(result, value.frames_received);
    append_little(result, value.frames_presented);
    append_little(result, value.frames_dropped);
    append_little(result, value.overlays_suppressed);
    append_little(result, value.patches_received);
    append_little(result, value.patches_presented);
    append_little(result, value.patches_rejected);
    return result;
}

void append_ring(std::vector<std::byte>& output, const SharedPcmRing* ring) {
    append_little(output, static_cast<std::uint32_t>(ring != nullptr));
    if (!ring) {
        for (int index = 0; index < 9; ++index) append_little(output, std::uint32_t{});
        return;
    }
    const auto& format = ring->format();
    append_little(output, format.sample_rate);
    append_little(output, static_cast<std::uint32_t>(format.channels));
    append_little(output, static_cast<std::uint32_t>(format.bits_per_sample));
    append_little(output, static_cast<std::uint32_t>(format.block_align));
    append_little(output, static_cast<std::uint32_t>(format.sample_kind));
    append_little(output, ring->capacity_frames());
    append_little(output, ring->available_frames());
    append_little(output, ring->overflow_frames());
    append_little(output, ring->underflow_frames());
}

[[nodiscard]] std::vector<std::byte> audio_payload(IMediaPlatform& platform) {
    std::vector<std::byte> result;
    result.reserve(128);
    append_little(result, std::uint32_t{1});
    append_little(result, std::uint32_t{0}); // cross-process mapping is not negotiated
    append_ring(result, platform.capture_pcm_ring());
    append_ring(result, platform.render_pcm_ring());
    return result;
}

#ifdef _WIN32
[[nodiscard]] ipc::AudioOutputEndpoint audio_output_endpoint(
    const windows::AudioOutputEndpoint& endpoint) {
    return {endpoint.endpoint_id, endpoint.friendly_name,
            static_cast<ipc::AudioOutputState>(endpoint.state), endpoint.system_default,
            endpoint.generation};
}

[[nodiscard]] ipc::SelectedAudioOutput selected_audio_output(
    const windows::SelectedAudioOutput& selected) {
    return {selected.schema_version, selected.selection.mode,
            selected.selection.endpoint_id, audio_output_endpoint(selected.resolved)};
}

[[nodiscard]] ipc::AudioInputEndpoint audio_input_endpoint(
    const windows::AudioInputEndpoint& endpoint) {
    return {endpoint.endpoint_id, endpoint.friendly_name,
            static_cast<ipc::AudioInputState>(endpoint.state), endpoint.system_default,
            endpoint.generation};
}

[[nodiscard]] ipc::SelectedAudioInput selected_audio_input(
    const windows::SelectedAudioInput& selected) {
    return {selected.schema_version, selected.selection.mode,
            selected.selection.endpoint_id, audio_input_endpoint(selected.resolved)};
}

[[nodiscard]] ipc::TrustedSubtitlePresentationContext presentation_context(
    const windows::TrustedSubtitlePresentationContext& context) {
    return {
        context.schema_version,
        context.selected_process_id,
        context.selected_window,
        context.selected_executable_name,
        context.capture_device_generation,
        context.geometry_epoch,
        context.source_frame_sequence,
        context.source_frame_qpc,
        context.window_bounds_px,
        context.client_bounds_px,
        context.captured_content_px,
        context.monitor_id,
        context.monitor_bounds_px,
        context.monitor_work_area_px,
        context.dpi_available,
        context.dpi_x,
        context.dpi_y,
        context.hdr_evidence_available,
        context.hdr_supported,
        context.hdr_user_enabled,
        context.hdr_active,
        context.advanced_color_active,
        context.active_color_mode,
        context.color_encoding_available,
        context.color_encoding,
        context.bits_per_color_channel,
        context.sdr_white_level_available,
        context.sdr_white_level_nits,
        context.capture_backend,
        context.capture_scope,
        context.overlay_capture_excluded,
        context.overlay_visuals_allowed,
        context.target_color_space_available,
        context.target_color_space,
        context.attested_at_qpc,
        context.qpc_frequency,
        context.attestation_id,
    };
}

[[nodiscard]] MonotonicTime qpc_to_monotonic(const std::uint64_t timestamp,
                                             const std::uint64_t now_qpc,
                                             const std::uint64_t frequency,
                                             const MonotonicTime now) {
    if (timestamp >= now_qpc || frequency == 0) return now;
    const auto nanoseconds = static_cast<std::uint64_t>(
        (static_cast<long double>(now_qpc - timestamp) * 1'000'000'000.0L) /
        static_cast<long double>(frequency));
    return now - std::chrono::nanoseconds(nanoseconds);
}

[[nodiscard]] ipc::Response process_command(const ipc::Envelope& envelope,
                                            const ipc::Command& command,
                                            MediaBroker& broker,
                                            windows::PlaybackService& playback_service,
                                            windows::InputRehearsalService& input_service,
                                            const std::shared_ptr<windows::PttActivationTracker>& ptt_tracker,
                                            std::optional<ipc::SubmitOcclusionCommand>& residual_occlusion,
                                            bool& shutdown,
                                            const std::uint64_t now_qpc,
                                            const std::uint64_t frequency) {
    ipc::Response response{ipc::protocol_version, envelope.sequence, ipc::StatusCode::ok,
                           broker.diagnostics().cancellation_generation, {}};
    if (std::holds_alternative<ipc::HealthCommand>(command)) {
        append_little(response.payload, now_qpc);
        append_little(response.payload, static_cast<std::uint32_t>(broker.diagnostics().state));
    } else if (const auto* select = std::get_if<ipc::SelectTargetCommand>(&command)) {
        residual_occlusion.reset();
        playback_service.cancel_all();
        input_service.cancel_all();
        broker.platform().cancel_visual_source_leases();
        broker.platform().cancel_identity_frame_leases();
        broker.platform().cancel_manual_actor_picker();
        const auto evidence = windows::inspect_target(
            reinterpret_cast<HWND>(static_cast<std::uintptr_t>(select->native_window)),
            select->expected_process_id);
        const auto decision = evaluate_target_policy(evidence, select->allowed_process_names);
        if (!decision.capture_allowed) {
            broker.clear_target();
            response.status = ipc::StatusCode::target_blocked;
            append_little(response.payload, static_cast<std::uint32_t>(decision.reason));
        } else {
            GameTarget target{static_cast<std::uintptr_t>(select->native_window), evidence.process_id,
                              evidence.process_name, evidence.process_name};
            if (!broker.select_target(std::move(target))) response.status = ipc::StatusCode::target_blocked;
        }
    } else if (std::holds_alternative<ipc::ClearTargetCommand>(command)) {
        residual_occlusion.reset();
        playback_service.cancel_all();
        input_service.cancel_all();
        broker.platform().cancel_visual_source_leases();
        broker.platform().cancel_identity_frame_leases();
        broker.platform().cancel_manual_actor_picker();
        broker.clear_target();
    } else if (const auto* ptt = std::get_if<ipc::ConfigurePttCommand>(&command)) {
        Failure failure;
        broker.platform().unregister_ptt_hotkey();
        if (!broker.platform().register_ptt_hotkey(ptt->virtual_key, failure)) {
            response.status = ipc::StatusCode::payload_invalid;
        } else {
            ptt_tracker->configure(ptt->virtual_key, now_qpc);
        }
    } else if (std::holds_alternative<ipc::AudioStatusCommand>(command)) {
        response.payload = audio_payload(broker.platform());
        response.status = ipc::StatusCode::capability_unavailable;
    } else if (const auto* occlusion = std::get_if<ipc::SubmitOcclusionCommand>(&command)) {
        const auto& diagnostics = broker.diagnostics();
        const bool source_frame_recent = frequency > 0U &&
            occlusion->source_frame_sequence > 0U &&
            occlusion->source_frame_sequence <= diagnostics.latest_frame_sequence &&
            occlusion->source_frame_qpc > 0U &&
            occlusion->source_frame_qpc <= diagnostics.latest_frame_qpc &&
            occlusion->source_frame_qpc <= now_qpc + frequency * 2U / 1000U &&
            occlusion->source_frame_qpc +
                    frequency * visual_presentation_deadline_ms / 1000U >= now_qpc;
        if (!source_frame_recent ||
            occlusion->source_device_generation != diagnostics.device_generation ||
            occlusion->source_geometry_epoch != diagnostics.geometry_epoch) {
            response.status = ipc::StatusCode::payload_invalid;
        } else {
            const auto now = std::chrono::steady_clock::now();
            broker.submit_occlusion_evidence({
                occlusion->face_confidence,
                occlusion->landmark_confidence,
                occlusion->visibility_ratio,
                occlusion->mouth_occluded,
                qpc_to_monotonic(occlusion->measured_qpc, now_qpc, frequency, now),
                occlusion->source_frame_sequence,
                occlusion->source_device_generation,
                occlusion->source_geometry_epoch,
                occlusion->source_frame_qpc,
                occlusion->actor_id,
                occlusion->track_id,
                occlusion->track_epoch,
            });
            residual_occlusion = *occlusion;
        }
    } else if (const auto* patch = std::get_if<ipc::SubmitPatchCommand>(&command)) {
        const auto& diagnostics = broker.diagnostics();
        const ipc::ResidualValidationContext context{
            envelope.nonce,
            envelope.session_id,
            GetCurrentProcessId(),
            diagnostics.selected_process_id,
            diagnostics.cancellation_generation,
            diagnostics.device_generation,
            diagnostics.geometry_epoch,
            diagnostics.latest_frame_sequence,
            diagnostics.latest_frame_qpc,
            diagnostics.latest_content_size_px,
            now_qpc,
            frequency,
            diagnostics.capture_backend,
            diagnostics.overlay_visuals_allowed,
        };
        const auto contract = ipc::validate_residual_contract(*patch, context);
        append_little(response.payload, static_cast<std::uint32_t>(contract));
        const bool occlusion_current = residual_occlusion &&
            !residual_occlusion->mouth_occluded &&
            residual_occlusion->face_confidence >= 0.78 &&
            residual_occlusion->landmark_confidence >= 0.82 &&
            residual_occlusion->visibility_ratio >= 0.72 &&
            residual_occlusion->source_frame_sequence == patch->source_frame_sequence &&
            residual_occlusion->source_device_generation == patch->source_device_generation &&
            residual_occlusion->source_geometry_epoch == patch->source_geometry_epoch &&
            residual_occlusion->source_frame_qpc == patch->source_frame_qpc &&
            residual_occlusion->actor_id == patch->actor_id &&
            residual_occlusion->track_id == patch->track_id &&
            residual_occlusion->track_epoch == patch->track_epoch &&
            residual_occlusion->measured_qpc <= now_qpc + frequency * 2U / 1000U &&
            residual_occlusion->measured_qpc + frequency * 120U / 1000U >= now_qpc;
        residual_occlusion.reset();
        if (contract != ipc::ResidualContractStatus::accepted || !patch->shared_texture ||
            !occlusion_current) {
            broker.platform().release_shared_residual();
            broker.platform().suppress_residual();
            response.status = ipc::StatusCode::payload_invalid;
            append_little(response.payload, std::uint32_t{0});
        } else {
            const auto& texture = *patch->shared_texture;
            SharedResidualLease lease{
                texture.schema_version,
                texture.worker_process_id,
                texture.worker_process_creation_time,
                texture.worker_executable_name,
                texture.source_process_handle_value,
                texture.lease_nonce_high,
                texture.lease_nonce_low,
                texture.adapter_luid,
                texture.keyed_mutex_acquire_key,
                texture.keyed_mutex_release_key,
                texture.width,
                texture.height,
                texture.stride_bytes,
                texture.dxgi_format,
                texture.alpha_mode,
                texture.expires_qpc,
                patch->cancellation_generation,
                patch->source_device_generation,
                patch->source_geometry_epoch,
                patch->source_frame_sequence,
                patch->source_frame_qpc,
                patch->produced_qpc,
                patch->actor_id,
                patch->track_id,
                patch->track_epoch,
                {patch->left, patch->top, patch->right, patch->bottom},
            };
            std::uintptr_t native_texture{};
            Failure import_failure;
            if (!broker.platform().import_shared_residual(lease, native_texture, import_failure) ||
                native_texture == 0) {
                broker.platform().release_shared_residual();
                broker.platform().suppress_residual();
                response.status = ipc::StatusCode::capability_unavailable;
                append_little(response.payload, std::uint32_t{0});
                append_little(response.payload, static_cast<std::uint32_t>(import_failure.code));
            } else {
                const auto now = std::chrono::steady_clock::now();
                const MouthPatch mouth_patch{
                    patch->source_frame_sequence,
                    patch->cancellation_generation,
                    {patch->left, patch->top, patch->right, patch->bottom},
                    patch->confidence,
                    qpc_to_monotonic(patch->produced_qpc, now_qpc, frequency, now),
                    native_texture,
                    patch->source_device_generation,
                    qpc_to_monotonic(patch->source_frame_qpc, now_qpc, frequency, now),
                    patch->source_geometry_epoch,
                    patch->source_frame_qpc,
                    patch->actor_id,
                    patch->track_id,
                    patch->track_epoch,
                };
                Failure presentation_failure;
                const bool presented = broker.platform().present_shared_residual(
                    mouth_patch, presentation_failure);
                append_little(response.payload, static_cast<std::uint32_t>(presented));
                if (!presented) {
                    broker.platform().release_shared_residual();
                    broker.platform().suppress_residual();
                    response.status = ipc::StatusCode::capability_unavailable;
                    append_little(response.payload,
                                  static_cast<std::uint32_t>(presentation_failure.code));
                }
            }
        }
    } else if (const auto* cancel = std::get_if<ipc::CancelCommand>(&command)) {
        if (cancel->new_generation <= broker.diagnostics().cancellation_generation) {
            response.status = ipc::StatusCode::cancellation_mismatch;
        } else {
            // Validate and advance the authoritative generation before touching
            // any in-flight media. A replayed/malformed cancel must have no
            // side effects, and terminal receipts from the old generation must
            // already be stale when teardown competes with pointer completion.
            broker.cancel_generation(cancel->new_generation);
            response.cancellation_generation = broker.diagnostics().cancellation_generation;
            residual_occlusion.reset();
            playback_service.cancel_all();
            input_service.cancel_all();
            broker.platform().cancel_visual_source_leases();
            broker.platform().cancel_identity_frame_leases();
            broker.platform().cancel_manual_actor_picker();
        }
    } else if (const auto* visual_allocation =
                   std::get_if<ipc::AllocateVisualSourceCommand>(&command)) {
        const auto& diagnostics = broker.diagnostics();
        VisualSourceLease lease;
        Failure failure;
        const VisualSourceLeaseRequest request{
            diagnostics.cancellation_generation,
            visual_allocation->worker_process_id,
            visual_allocation->worker_process_creation_time,
            visual_allocation->worker_executable_name,
            visual_allocation->actor_id,
            visual_allocation->track_id,
            visual_allocation->track_epoch,
            visual_allocation->reserve_for_actor_picker,
        };
        if (!broker.platform().allocate_visual_source(request, lease, failure)) {
            response.status = failure.code == FailureCode::timeout
                ? ipc::StatusCode::deadline_expired
                : ipc::StatusCode::capability_unavailable;
            append_little(response.payload, static_cast<std::uint32_t>(failure.code));
        } else {
            const auto encoded = ipc::encode_visual_source_lease(lease);
            if (!encoded) {
                broker.platform().cancel_visual_source_leases();
                response.status = ipc::StatusCode::internal_error;
            } else {
                response.payload = *encoded;
            }
        }
    } else if (const auto* visual_release =
                   std::get_if<ipc::ReleaseVisualSourceCommand>(&command)) {
        if (!broker.platform().release_visual_source(visual_release->worker_process_id,
                                                     visual_release->lease_nonce_high,
                                                     visual_release->lease_nonce_low)) {
            response.status = ipc::StatusCode::payload_invalid;
        }
    } else if (const auto* identity_allocation =
                   std::get_if<ipc::AllocateIdentityFrameCommand>(&command)) {
        const auto& diagnostics = broker.diagnostics();
        IdentityFrameLease lease;
        Failure failure;
        const IdentityFrameLeaseRequest request{
            envelope.session_id,
            diagnostics.cancellation_generation,
            identity_allocation->worker_process_id,
            identity_allocation->worker_process_creation_time,
            identity_allocation->worker_executable_name,
            identity_allocation->crop_px,
        };
        if (!broker.platform().allocate_identity_frame(request, lease, failure)) {
            response.status = failure.code == FailureCode::timeout
                ? ipc::StatusCode::deadline_expired
                : ipc::StatusCode::capability_unavailable;
            append_little(response.payload, static_cast<std::uint32_t>(failure.code));
        } else {
            const auto encoded = ipc::encode_identity_frame_lease(lease);
            if (!encoded) {
                broker.platform().cancel_identity_frame_leases();
                response.status = ipc::StatusCode::internal_error;
            } else {
                response.payload = *encoded;
            }
        }
    } else if (const auto* identity_release =
                   std::get_if<ipc::ReleaseIdentityFrameCommand>(&command)) {
        if (!broker.platform().release_identity_frame(identity_release->worker_process_id,
                                                       identity_release->lease_id,
                                                       identity_release->lease_nonce)) {
            response.status = ipc::StatusCode::payload_invalid;
        }
    } else if (const auto* reference_allocation =
                   std::get_if<ipc::AllocateIdentityReferenceImportCommand>(&command)) {
        const auto& diagnostics = broker.diagnostics();
        IdentityReferenceImportLease lease;
        Failure failure;
        const IdentityReferenceImportRequest request{
            envelope.session_id,
            diagnostics.cancellation_generation,
            reference_allocation->worker_process_id,
            reference_allocation->worker_process_creation_time,
            reference_allocation->worker_executable_name,
            reference_allocation->picker_consent_token,
            reference_allocation->game_profile_id,
            reference_allocation->character_id,
            reference_allocation->subject_id,
            reference_allocation->reference_id,
            reference_allocation->subject_display_name,
            reference_allocation->source_class,
            reference_allocation->owner_user_id,
            reference_allocation->original_work_license,
            reference_allocation->explicit_user_consent,
            reference_allocation->local_only,
            reference_allocation->imported_at_unix_ms,
        };
        if (!broker.platform().allocate_identity_reference_import(request, lease, failure)) {
            response.status = failure.code == FailureCode::timeout
                ? ipc::StatusCode::deadline_expired
                : ipc::StatusCode::capability_unavailable;
            append_little(response.payload, static_cast<std::uint32_t>(failure.code));
        } else {
            const auto encoded = ipc::encode_identity_reference_import_lease(lease);
            if (!encoded) {
                broker.platform().cancel_identity_frame_leases();
                response.status = ipc::StatusCode::internal_error;
            } else {
                response.payload = *encoded;
            }
        }
    } else if (const auto* reference_release =
                   std::get_if<ipc::ReleaseIdentityReferenceImportCommand>(&command)) {
        if (!broker.platform().release_identity_reference_import(
                reference_release->worker_process_id,
                reference_release->lease_id,
                reference_release->lease_nonce)) {
            response.status = ipc::StatusCode::payload_invalid;
        }
    } else if (std::holds_alternative<ipc::EnumerateAudioOutputsCommand>(command)) {
        std::string error;
        const auto snapshot = playback_service.enumerate_audio_outputs(error);
        if (!snapshot) {
            response.status = ipc::StatusCode::capability_unavailable;
        } else {
            ipc::AudioOutputSnapshot wire;
            wire.schema_version = snapshot->schema_version;
            wire.catalog_generation = snapshot->catalog_generation;
            wire.endpoints.reserve(snapshot->endpoints.size());
            for (const auto& endpoint : snapshot->endpoints) {
                wire.endpoints.push_back(audio_output_endpoint(endpoint));
            }
            const auto payload = ipc::encode_audio_output_snapshot(wire);
            if (payload) response.payload = *payload;
            else response.status = ipc::StatusCode::internal_error;
        }
    } else if (const auto* select_audio =
                   std::get_if<ipc::SelectAudioOutputCommand>(&command)) {
        std::string error;
        const auto selected = playback_service.select_audio_output(
            {select_audio->mode, select_audio->endpoint_id}, error);
        const auto payload = selected
            ? ipc::encode_selected_audio_output(selected_audio_output(*selected))
            : std::nullopt;
        if (payload) response.payload = *payload;
        else response.status = selected ? ipc::StatusCode::internal_error
                                        : ipc::StatusCode::capability_unavailable;
    } else if (std::holds_alternative<ipc::SelectedAudioOutputCommand>(command)) {
        std::string error;
        const auto selected = playback_service.selected_audio_output(error);
        const auto payload = selected
            ? ipc::encode_selected_audio_output(selected_audio_output(*selected))
            : std::nullopt;
        if (payload) response.payload = *payload;
        else response.status = selected ? ipc::StatusCode::internal_error
                                        : ipc::StatusCode::capability_unavailable;
    } else if (std::holds_alternative<
                   ipc::TrustedSubtitlePresentationContextCommand>(command)) {
        std::string error;
        auto opening = broker.diagnostics();
        auto context = windows::query_trusted_subtitle_presentation_context(opening, error);
        const auto same_attested_target = [](const Diagnostics& left, const Diagnostics& right) {
            return left.selected_process_id == right.selected_process_id &&
                   left.selected_window == right.selected_window &&
                   left.selected_executable_name == right.selected_executable_name &&
                   left.device_generation == right.device_generation &&
                   left.geometry_epoch == right.geometry_epoch &&
                   left.capture_backend == right.capture_backend &&
                   left.overlay_capture_excluded == right.overlay_capture_excluded &&
                   left.overlay_visuals_allowed == right.overlay_visuals_allowed;
        };
        auto closing = broker.diagnostics();
        if (context && same_attested_target(opening, closing) &&
            (opening.latest_frame_sequence != closing.latest_frame_sequence ||
             opening.latest_frame_qpc != closing.latest_frame_qpc)) {
            opening = closing;
            context = windows::query_trusted_subtitle_presentation_context(opening, error);
            closing = broker.diagnostics();
        }
        if (!context || !same_attested_target(opening, closing) ||
            context->source_frame_sequence != opening.latest_frame_sequence ||
            context->source_frame_qpc != opening.latest_frame_qpc) {
            context.reset();
        }
        const auto payload = context
            ? ipc::encode_trusted_subtitle_presentation_context(
                  presentation_context(*context))
            : std::nullopt;
        if (payload) response.payload = *payload;
        else response.status = context ? ipc::StatusCode::internal_error
                                       : ipc::StatusCode::capability_unavailable;
    } else if (const auto* allocation =
                   std::get_if<ipc::AllocatePlaybackStreamCommand>(&command)) {
        const playback::AllocationRequest request{
            allocation->session_id,
            allocation->turn_id,
            allocation->generation,
            allocation->sample_rate,
            allocation->channels,
            allocation->max_frames,
            allocation->expected_producer_process_id,
        };
        std::string error;
        const auto lease = playback_service.allocate(request, now_qpc, frequency, error);
        const auto payload = lease ? ipc::encode_playback_lease(*lease) : std::nullopt;
        if (!lease || !payload) {
            response.status = ipc::StatusCode::capability_unavailable;
        } else {
            response.payload = *payload;
        }
    } else if (std::holds_alternative<ipc::CancelPlaybackCommand>(command)) {
        playback_service.cancel_all();
    } else if (std::holds_alternative<ipc::EnumerateAudioInputsCommand>(command)) {
        std::string error;
        const auto snapshot = input_service.enumerate_audio_inputs(error);
        if (!snapshot) {
            response.status = ipc::StatusCode::capability_unavailable;
        } else {
            ipc::AudioInputSnapshot wire;
            wire.schema_version = snapshot->schema_version;
            wire.catalog_generation = snapshot->catalog_generation;
            wire.endpoints.reserve(snapshot->endpoints.size());
            for (const auto& endpoint : snapshot->endpoints) {
                wire.endpoints.push_back(audio_input_endpoint(endpoint));
            }
            const auto payload = ipc::encode_audio_input_snapshot(wire);
            if (payload) response.payload = *payload;
            else response.status = ipc::StatusCode::internal_error;
        }
    } else if (const auto* select_input =
                   std::get_if<ipc::SelectAudioInputCommand>(&command)) {
        std::string error;
        const auto selected = input_service.select_audio_input(
            {select_input->mode, select_input->endpoint_id}, error);
        const auto payload = selected
            ? ipc::encode_selected_audio_input(selected_audio_input(*selected))
            : std::nullopt;
        if (payload) response.payload = *payload;
        else response.status = selected ? ipc::StatusCode::internal_error
                                        : ipc::StatusCode::capability_unavailable;
    } else if (std::holds_alternative<ipc::SelectedAudioInputCommand>(command)) {
        std::string error;
        const auto selected = input_service.selected_audio_input(error);
        const auto payload = selected
            ? ipc::encode_selected_audio_input(selected_audio_input(*selected))
            : std::nullopt;
        if (payload) response.payload = *payload;
        else response.status = selected ? ipc::StatusCode::internal_error
                                        : ipc::StatusCode::capability_unavailable;
    } else if (const auto* rehearsal =
                   std::get_if<ipc::AllocateAudioInputRehearsalCommand>(&command)) {
        std::string error;
        const auto lease = input_service.allocate(
            {rehearsal->session_id, rehearsal->turn_id, rehearsal->generation,
             rehearsal->duration_ms, rehearsal->sample_rate, rehearsal->channels,
             rehearsal->max_frames,
             rehearsal->expected_producer_process_id,
             rehearsal->activation_source},
            now_qpc, frequency, error);
        const auto payload = lease ? ipc::encode_input_rehearsal_lease(*lease)
                                   : std::nullopt;
        if (payload) response.payload = *payload;
        else response.status = lease ? ipc::StatusCode::internal_error
                                     : ipc::StatusCode::capability_unavailable;
    } else if (const auto* cancel_rehearsal =
                   std::get_if<ipc::CancelAudioInputRehearsalCommand>(&command)) {
        if (!input_service.cancel(cancel_rehearsal->stream_id,
                                  cancel_rehearsal->generation)) {
            response.status = ipc::StatusCode::payload_invalid;
        }
    } else if (const auto* query =
                   std::get_if<ipc::QueryVisualAudioEnvelopeCommand>(&command)) {
        const auto value = playback_service.query_visual_audio_envelope(
            query->session_id, query->turn_id, query->generation,
            query->stream_id, query->segment_id);
        if (!value) {
            response.status = ipc::StatusCode::capability_unavailable;
        } else {
            ipc::VisualAudioEnvelope wire;
            wire.schema_version = value->schema_version;
            wire.session_id = value->session_id;
            wire.turn_id = value->turn_id;
            wire.generation = value->generation;
            wire.stream_id = value->stream_id;
            wire.segment_id = value->segment_id;
            wire.source_sample_start = value->source_sample_start;
            wire.source_sample_count = value->source_sample_count;
            wire.sample_rate = value->sample_rate;
            wire.channels = value->channels;
            wire.device_write_qpc = value->device_write_qpc;
            wire.qpc_frequency = value->qpc_frequency;
            wire.source_frames = value->source_frames;
            wire.device_frames = value->device_frames;
            wire.mono_rms_q15 = value->mono_rms_q15;
            wire.mono_peak_q15 = value->mono_peak_q15;
            wire.visual_speech_cues = value->visual_speech_cues;
            wire.active = value->active;
            wire.draining = value->draining;
            wire.cancelled = value->cancelled;
            const auto payload = ipc::encode_visual_audio_envelope(wire);
            if (payload) response.payload = *payload;
            else response.status = ipc::StatusCode::internal_error;
        }
    } else if (std::holds_alternative<ipc::QueryPttActivationStateCommand>(command)) {
        const auto snapshot = ptt_tracker ? ptt_tracker->snapshot()
                                          : windows::PttActivationSnapshot{};
        const ipc::PttActivationState wire{
            1, snapshot.virtual_key, snapshot.state,
            snapshot.transition_sequence, snapshot.transition_qpc,
            snapshot.release_transition_sequence, snapshot.released_qpc};
        const auto payload = ipc::encode_ptt_activation_state(wire);
        if (payload) response.payload = *payload;
        else response.status = ipc::StatusCode::capability_unavailable;
    } else if (const auto* picker =
                   std::get_if<ipc::ManualActorPickerCommand>(&command)) {
        ManualActorPickerReceipt receipt;
        Failure failure;
        bool succeeded{};
        if (picker->action == ipc::ManualActorPickerAction::begin) {
            const auto& diagnostics = broker.diagnostics();
            const ManualActorPickerRequest request{
                picker->request_id,
                envelope.session_id,
                diagnostics.cancellation_generation,
                diagnostics.selected_process_id,
                static_cast<std::uint64_t>(diagnostics.selected_window),
                picker->source_device_generation,
                picker->source_geometry_epoch,
                picker->source_frame_sequence,
                picker->source_frame_qpc,
                picker->timeout_ms,
                picker->candidates,
            };
            succeeded = broker.platform().begin_manual_actor_picker(request, receipt, failure);
        } else if (picker->action == ipc::ManualActorPickerAction::poll) {
            succeeded = broker.platform().query_manual_actor_picker(
                picker->request_id, receipt, failure);
        } else {
            succeeded = broker.platform().cancel_manual_actor_picker(
                picker->request_id, receipt, failure);
        }
        const auto payload = succeeded
            ? ipc::encode_manual_actor_picker_receipt(receipt)
            : std::nullopt;
        if (payload) {
            response.payload = *payload;
        } else {
            response.status = failure.code == FailureCode::timeout
                ? ipc::StatusCode::deadline_expired
                : succeeded ? ipc::StatusCode::internal_error
                            : ipc::StatusCode::capability_unavailable;
            append_little(response.payload, static_cast<std::uint32_t>(failure.code));
        }
    } else if (std::holds_alternative<ipc::DiagnosticsCommand>(command)) {
        response.payload = diagnostics_payload(broker.diagnostics());
    } else if (std::holds_alternative<ipc::CaptureEvidenceCommand>(command)) {
        const auto& evidence = broker.diagnostics();
        const auto pixel_source = evidence.capture_backend == CaptureBackend::windows_graphics_capture
                                      ? CapturePixelSource::windows_graphics_capture_texture
                                  : evidence.capture_backend == CaptureBackend::desktop_duplication
                                      ? CapturePixelSource::desktop_duplication_texture
                                      : CapturePixelSource::unavailable;
        const auto pixel_scope = evidence.capture_backend == CaptureBackend::windows_graphics_capture
                                     ? CapturePixelScope::exact_selected_window
                                 : evidence.capture_backend == CaptureBackend::desktop_duplication
                                     ? CapturePixelScope::full_display_output
                                     : CapturePixelScope::unavailable;
        const bool exact_window_overlay_independence =
            pixel_source == CapturePixelSource::windows_graphics_capture_texture &&
            pixel_scope == CapturePixelScope::exact_selected_window;
        append_little(response.payload, std::uint32_t{3});
        append_little(response.payload, evidence.selected_process_id);
        append_little(response.payload, static_cast<std::uint64_t>(evidence.selected_window));
        append_little(response.payload, evidence.device_generation);
        append_little(response.payload, evidence.geometry_epoch);
        append_little(response.payload, evidence.latest_frame_sequence);
        append_little(response.payload, evidence.latest_frame_qpc);
        append_little(response.payload, evidence.initial_content_hash);
        append_little(response.payload, evidence.latest_content_hash);
        append_little(response.payload, evidence.content_hash_changes);
        append_little(response.payload, evidence.geometry_changes);
        append_little(response.payload, evidence.nonadvancing_frames);
        append_little(response.payload, static_cast<std::uint32_t>(evidence.latest_content_size_px.width));
        append_little(response.payload, static_cast<std::uint32_t>(evidence.latest_content_size_px.height));
        append_little(response.payload, static_cast<std::uint32_t>(evidence.overlay_capture_excluded));
        append_little(response.payload, static_cast<std::uint32_t>(evidence.overlay_visuals_allowed));
        append_little(response.payload, static_cast<std::uint32_t>(evidence.selected_executable_name.size()));
        append_little(response.payload, static_cast<std::uint32_t>(pixel_source));
        append_little(response.payload, static_cast<std::uint32_t>(pixel_scope));
        append_little(response.payload, static_cast<std::uint32_t>(exact_window_overlay_independence));
        append_little(response.payload, static_cast<std::uint32_t>(exact_window_overlay_independence));
        // Privacy-safe environment caveat only. The product does not enumerate,
        // name, count, focus, or otherwise inspect third-party display overlays.
        append_little(response.payload, std::uint32_t{1});
        response.payload.insert(response.payload.end(),
                                reinterpret_cast<const std::byte*>(evidence.selected_executable_name.data()),
                                reinterpret_cast<const std::byte*>(evidence.selected_executable_name.data() +
                                                                   evidence.selected_executable_name.size()));
    } else if (std::holds_alternative<ipc::ShutdownCommand>(command)) {
        shutdown = true;
    }
    // Target selection, clearing, and cancellation invalidate in-flight visual
    // work. Return the post-command generation so an authenticated controller
    // can issue its next envelope without guessing how the broker advanced it.
    response.cancellation_generation = broker.diagnostics().cancellation_generation;
    return response;
}
#endif

} // namespace

int main(int argc, char** argv) {
    std::vector<std::string_view> arguments;
    arguments.reserve(argc > 1 ? static_cast<std::size_t>(argc - 1) : 0);
    for (int index = 1; index < argc; ++index) arguments.emplace_back(argv[index]);
    const auto config = parse_service_launch_arguments(arguments);
    if (!config) {
        std::cerr << "media service launch context is missing or malformed\n";
        return EXIT_FAILURE;
    }

#ifndef _WIN32
    std::cerr << "media service is supported only on Windows\n";
    return EXIT_FAILURE;
#else
    HANDLE parent{};
    if (!windows::verify_parent_and_job(*config, parent)) {
        std::cerr << "media service parent or Job Object verification failed\n";
        return EXIT_FAILURE;
    }
    auto ptt_tracker = std::make_shared<windows::PttActivationTracker>();
    BrokerEventSink event_sink;
    event_sink.on_ptt = [ptt_tracker](const PttState state, const MonotonicTime) {
        ptt_tracker->transition(state, windows::qpc_now());
    };
    MediaBroker broker(create_windows_media_platform(), {}, std::move(event_sink));
    if (!broker.start()) {
        CloseHandle(parent);
        std::cerr << "media service initialization failed\n";
        return EXIT_FAILURE;
    }
    windows::CurrentUserPipeServer pipe(config->pipe_name, config->parent_process_id);
    windows::PlaybackService playback_service(config->session_id);
    ptt_tracker->configure(0x77, windows::qpc_now());
    windows::InputRehearsalService input_service(config->session_id, ptt_tracker);
    if (!pipe.start()) {
        broker.stop();
        CloseHandle(parent);
        std::cerr << "media service control endpoint failed\n";
        return EXIT_FAILURE;
    }

    const auto frequency = windows::qpc_frequency();
    ipc::EnvelopeValidator validator({config->nonce, config->session_id, frequency * 10});
    std::optional<ipc::SubmitOcclusionCommand> residual_occlusion;
    bool shutdown{};
    while (!shutdown) {
        if (WaitForSingleObject(parent, 0) == WAIT_OBJECT_0 || pipe.disconnected()) break;
        broker.tick();
        playback_service.reap_finished();
        input_service.reap_finished();
        if (auto envelope = pipe.take_request()) {
            const auto now_qpc = windows::qpc_now();
            const auto status = validator.validate(*envelope, now_qpc,
                                                   broker.diagnostics().cancellation_generation);
            ipc::Response response{ipc::protocol_version, envelope->sequence, status,
                                   broker.diagnostics().cancellation_generation, {}};
            if (status == ipc::StatusCode::ok) {
                auto command = ipc::decode_command(envelope->command, envelope->payload);
                response = command ? process_command(*envelope, *command, broker, playback_service,
                                                     input_service,
                                                     ptt_tracker,
                                                     residual_occlusion,
                                                     shutdown, now_qpc, frequency)
                                   : ipc::Response{ipc::protocol_version, envelope->sequence,
                                                   ipc::StatusCode::payload_invalid,
                                                   broker.diagnostics().cancellation_generation, {}};
            }
            const auto response_sequence = response.response_to_sequence;
            if (!pipe.submit_response(std::move(response))) break;
            if (shutdown) {
                const auto flush_deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(500);
                while (!pipe.response_sent(response_sequence) &&
                       std::chrono::steady_clock::now() < flush_deadline && !pipe.disconnected()) {
                    std::this_thread::sleep_for(std::chrono::milliseconds(1));
                }
            }
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(2));
    }
    pipe.stop();
    playback_service.cancel_all();
    input_service.cancel_all();
    broker.stop();
    CloseHandle(parent);
    return EXIT_SUCCESS;
#endif
}
