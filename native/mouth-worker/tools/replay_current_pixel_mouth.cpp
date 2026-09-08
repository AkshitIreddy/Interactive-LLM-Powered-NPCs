// Offline component replay for schema-4 current-pixel mouth rendering.
// Every residual is bound to the exact source frame and a real full-66 packet.
// Missing, occluded, rejected, and rate-limited packets preserve source pixels.
#include "headless_proof_support.hpp"

namespace {

struct ReplayPackets {
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t fps{};
    std::vector<std::optional<OpenSeeFaceLandmarkPacketV1>> packets;
};

// This intentionally matches replay_tracked_mouth.cpp's strict full-66 wire
// parser. The TSV is a conversion of retained model packets, not synthesized
// mouth geometry or a substitute landmark provider.
[[nodiscard]] ReplayPackets read_packets(const std::filesystem::path& path) {
    std::ifstream input(path);
    std::string magic;
    std::string line;
    std::size_t count{};
    ReplayPackets result;
    if (!(input >> magic >> result.width >> result.height >> count >> result.fps) ||
        magic != "npc-landmark-replay-v1" || result.fps != 30U ||
        count == 0U || count > 1'800U || result.width == 0U ||
        result.width > 4'096U || result.height == 0U || result.height > 4'096U) {
        throw std::runtime_error("invalid external landmark replay header");
    }
    std::getline(input, line);
    if (line.find_first_not_of(" \t\r") != std::string::npos) {
        throw std::runtime_error("extra replay header fields");
    }
    result.packets.reserve(count);
    for (std::size_t expected = 0U; expected < count; ++expected) {
        if (!std::getline(input, line)) {
            throw std::runtime_error("truncated landmark replay");
        }
        std::istringstream row(line);
        std::size_t index{};
        unsigned int valid{};
        unsigned int occluded{};
        if (!(row >> index >> valid) || index != expected || valid > 1U) {
            throw std::runtime_error("landmark replay indices must be contiguous");
        }
        OpenSeeFaceLandmarkPacketV1 packet{};
        if (valid != 0U) {
            if (!(row >> packet.detector_confidence >> packet.landmark_confidence >>
                  packet.visibility_ratio >> packet.pose.yaw >> packet.pose.pitch >>
                  packet.pose.roll >> packet.face_bounds.x >> packet.face_bounds.y >>
                  packet.face_bounds.width >> packet.face_bounds.height >> occluded) ||
                occluded > 1U) {
                throw std::runtime_error("invalid landmark replay geometry");
            }
            packet.mouth_occluded = occluded != 0U;
            for (auto& point : packet.landmarks) {
                if (!(row >> point.x >> point.y >> point.confidence)) {
                    throw std::runtime_error(
                        "landmark replay requires 66 complete points");
                }
            }
        }
        std::string extra;
        if (row >> extra) {
            throw std::runtime_error("extra landmark replay fields");
        }
        result.packets.push_back(
            valid != 0U ? std::optional(packet) : std::nullopt);
    }
    while (std::getline(input, line)) {
        if (line.find_first_not_of(" \t\r") != std::string::npos) {
            throw std::runtime_error("extra landmark replay rows");
        }
    }
    return result;
}

[[nodiscard]] std::size_t ppm_file_count(const std::filesystem::path& root) {
    if (!std::filesystem::is_directory(root)) {
        throw std::runtime_error("source frames must be a PPM directory");
    }
    std::size_t count{};
    for (const auto& entry : std::filesystem::directory_iterator(root)) {
        if (!entry.is_regular_file()) {
            continue;
        }
        auto extension = entry.path().extension().string();
        std::transform(extension.begin(), extension.end(), extension.begin(),
                       [](const unsigned char value) {
                           return static_cast<char>(std::tolower(value));
                       });
        count += extension == ".ppm" ? 1U : 0U;
    }
    if (count == 0U) {
        throw std::runtime_error("source directory contains no PPM frames");
    }
    return count;
}

[[nodiscard]] std::size_t changed_pixels_outside_support(
    const CpuFrame& source,
    const std::vector<std::uint8_t>& composited,
    const ResidualPatch& residual) {
    if (composited.size() != source.bgra.size()) {
        return source.lease.width * source.lease.height;
    }
    const auto left = static_cast<std::uint32_t>(std::llround(
        residual.normalized_bounds.x * static_cast<double>(source.lease.width)));
    const auto top = static_cast<std::uint32_t>(std::llround(
        residual.normalized_bounds.y * static_cast<double>(source.lease.height)));
    const auto right = left + residual.width;
    const auto bottom = top + residual.height;
    std::size_t changed{};
    for (std::uint32_t y = 0U; y < source.lease.height; ++y) {
        for (std::uint32_t x = 0U; x < source.lease.width; ++x) {
            if (x >= left && x < right && y >= top && y < bottom) {
                continue;
            }
            const auto offset = static_cast<std::size_t>(y) *
                                    source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            if (!std::equal(source.bgra.begin() + static_cast<std::ptrdiff_t>(offset),
                            source.bgra.begin() + static_cast<std::ptrdiff_t>(offset + 4U),
                            composited.begin() + static_cast<std::ptrdiff_t>(offset))) {
                ++changed;
            }
        }
    }
    return changed;
}

[[nodiscard]] std::string frame_name(const std::size_t index) {
    std::ostringstream name;
    name << "frame-" << std::setfill('0') << std::setw(5) << index << ".ppm";
    return name.str();
}

} // namespace

int main(int argc, char** argv) {
    try {
        if (argc != 7) {
            throw std::runtime_error(
                "usage: replay_current_pixel_mouth <source-ppm-directory> <audio.wav> "
                "<schema4-atlas|--source-only> <cues.tsv> <landmarks.tsv> <fresh-output>");
        }
        const std::filesystem::path source_root(argv[1]);
        const std::filesystem::path audio_path(argv[2]);
        const std::filesystem::path atlas_root(argv[3]);
        const std::filesystem::path cue_path(argv[4]);
        const std::filesystem::path landmark_path(argv[5]);
        const std::filesystem::path output(argv[6]);
        const bool source_only = std::string_view(argv[3]) == "--source-only";
        if (std::filesystem::exists(output)) {
            throw std::runtime_error("output must be a fresh directory");
        }

        const auto declared_ppm_frames = ppm_file_count(source_root);
        auto frames = read_source_frames(source_root);
        if (frames.size() != declared_ppm_frames) {
            throw std::runtime_error("source directory must contain only replayed PPM frames");
        }
        const auto audio = read_wav_pcm16(audio_path);
        const auto replay = read_packets(landmark_path);
        const auto audio_frames = audio.samples.size() / audio.channels;
        const auto expected_frames =
            (audio_frames * replay.fps + audio.sample_rate - 1U) / audio.sample_rate;
        if (frames.size() != replay.packets.size() ||
            frames.size() != expected_frames ||
            frames.front().lease.width != replay.width ||
            frames.front().lease.height != replay.height ||
            std::any_of(frames.begin(), frames.end(), [&](const CpuFrame& frame) {
                return frame.lease.width != replay.width ||
                       frame.lease.height != replay.height;
            })) {
            throw std::runtime_error(
                "audio, PPM frames, and full-66 geometry must cover one exact sequence");
        }

        const auto audio_digest = sha256_bytes(read_binary(audio_path));
        std::ifstream cue_stream(cue_path);
        const auto cues = read_review_cues(
            cue_stream, audio.sample_rate, audio_frames, audio_digest);

        constexpr std::uint64_t generation = 1U;
        const TrackBinding track{generation, 0x43555252454E54ULL,
                                 0x504958454C34ULL, 1U};
        ReferenceMouthWorker worker(generation);
        if (!source_only) {
            auto atlas = read_review_atlas(atlas_root, generation, track);
            if (atlas.schema_version != 4U) {
                throw std::runtime_error("current-pixel replay requires a schema-4 atlas");
            }
            if (!worker.install_atlas(std::move(atlas))) {
                throw std::runtime_error("schema-4 atlas failed native admission");
            }
        }

        OpenSeeFaceAdapterPolicy adapter_policy{};
        // The packet already belongs to this exact frame. Retaining the
        // adapter's safety gates is useful; geometry EMA would manufacture a
        // position between two packets and would weaken this replay claim.
        adapter_policy.smoothing_alpha = 1.0;
        OpenSeeFaceSignalAdapter adapter(generation, adapter_policy);
        adapter.set_current_frame_geometry(true);
        AppearanceGateEvidenceV1 appearance{};
        appearance.runtime_actor_id = track.actor_id;
        appearance.descriptor_revision = 1U;
        appearance.expected_descriptor_digest_high =
            appearance.observed_descriptor_digest_high = 1U;
        appearance.expected_descriptor_digest_low =
            appearance.observed_descriptor_digest_low = 2U;
        appearance.similarity = 1.0;
        appearance.temporal_iou = 1.0;
        appearance.identity_locked = true;
        appearance.target_visible = true;
        VisualResourceStateV1 resources{};
        resources.pressure = VisualPressure::nominal;
        resources.admitted_signal_rate_hz = 15U;
        resources.local_visuals_admitted = true;

        std::filesystem::create_directories(output / "frames");
        std::ofstream events(output / "frames.jsonl");
        if (!events) {
            throw std::runtime_error("could not create replay event ledger");
        }

        std::vector<double> adapter_times;
        std::vector<double> worker_times;
        std::vector<double> frame_times;
        std::size_t residual_frames{};
        std::size_t bypass_frames{};
        std::size_t changed_frames{};
        std::size_t source_exact_residual_frames{};
        std::size_t source_exact_bypass_frames{};
        std::size_t unavailable_packets{};
        std::size_t occluded_packets{};
        std::size_t unavailable_or_occluded_source_exact{};
        std::size_t adapter_bypasses{};
        std::size_t rate_limited_bypasses{};
        std::size_t worker_bypasses{};
        std::size_t outside_support_changed_pixels{};
        std::size_t bilabial_residual_frames{};
        std::size_t exact_bilabial_contact_frames{};

        std::size_t output_index{};
        for (std::size_t source_index = 0U; source_index < frames.size(); source_index += 2U) {
            auto& frame = frames[source_index];
            const auto at = 10'000'000'000LL +
                static_cast<Nanoseconds>(source_index) * 1'000'000'000LL /
                    static_cast<Nanoseconds>(replay.fps);
            frame.identity = {source_index + 1'000U, 1U, 1U, at};
            frame.lease.lease_nonce_low = source_index + 1U;
            frame.lease.expires_at_ns = at + 500'000'000LL;

            std::vector<std::uint8_t> pixels = frame.bgra;
            std::string reason = "external-detector-or-landmark-unavailable";
            bool has_residual{};
            bool packet_unavailable = !replay.packets[source_index].has_value();
            bool packet_occluded{};
            bool source_exact{};
            NormalizedRect bounds{};
            const auto first = source_index * audio.sample_rate / replay.fps;
            const Viseme viseme = review_viseme_at(cues, first);
            double adapter_ms{};
            double worker_ms{};

            if (packet_unavailable) {
                ++unavailable_packets;
            } else {
                auto packet = *replay.packets[source_index];
                packet_occluded = packet.mouth_occluded;
                occluded_packets += packet_occluded ? 1U : 0U;
                packet.provider_instance_id = 0x46554C4C3636ULL;
                packet.track = track;
                packet.frame = frame.identity;
                packet.source_frame_qpc = static_cast<std::uint64_t>(at);
                packet.qpc_frequency = 1'000'000'000U;
                packet.measured_at_ns = at;
                const auto adapter_started = std::chrono::steady_clock::now();
                const auto decision = adapter.adapt(
                    packet, appearance, resources, frame.identity, at + 1'000'000LL);
                adapter_ms = std::chrono::duration<double, std::milli>(
                    std::chrono::steady_clock::now() - adapter_started).count();
                adapter_times.push_back(adapter_ms);
                reason = std::string(to_string(decision.disposition));

                if (decision.accepted()) {
                    MouthDrive drive{};
                    drive.kind = DriveKind::timed_viseme;
                    // Match the product bridge: only the currently selected
                    // sample-bound cue snapshot is visible to the worker. The
                    // schema-4 trajectory retains motion state, not future cues.
                    drive.clock = {generation, 1U, first, 1U, first,
                                   audio.sample_rate, audio.channels, at};
                    drive.viseme = viseme;
                    drive.viseme_strength = 1.0;
                    if (!worker.submit({track, frame, *decision.tracking,
                                        std::move(drive), at + 100'000'000LL})) {
                        throw std::runtime_error("worker rejected exact-frame replay submission");
                    }
                    const auto worker_started = std::chrono::steady_clock::now();
                    auto result = worker.process_latest(
                        frame.identity, at + 2'000'000LL);
                    if (result.has_residual()) {
                        pixels = composite_over_source(frame, result.residual);
                        worker_ms = std::chrono::duration<double, std::milli>(
                            std::chrono::steady_clock::now() - worker_started).count();
                        outside_support_changed_pixels +=
                            changed_pixels_outside_support(frame, pixels, result.residual);
                        bounds = result.residual.normalized_bounds;
                        if (viseme == Viseme::bilabial) {
                            ++bilabial_residual_frames;
                            exact_bilabial_contact_frames +=
                                result.residual.coefficients.jaw_open <= 1.0e-9 &&
                                result.residual.coefficients.lip_close >= 1.0 - 1.0e-9
                                    ? 1U : 0U;
                        }
                        has_residual = true;
                        ++residual_frames;
                    } else {
                        worker_ms = std::chrono::duration<double, std::milli>(
                            std::chrono::steady_clock::now() - worker_started).count();
                        reason = std::string(to_string(result.disposition));
                        ++worker_bypasses;
                    }
                    worker_times.push_back(worker_ms);
                } else {
                    ++adapter_bypasses;
                    rate_limited_bypasses +=
                        decision.disposition == SignalDisposition::bypass_rate_limited
                            ? 1U : 0U;
                }
            }

            source_exact = pixels == frame.bgra;
            changed_frames += source_exact ? 0U : 1U;
            source_exact_residual_frames += has_residual && source_exact ? 1U : 0U;
            if (!has_residual) {
                worker.reset_current_pixel_history();
                ++bypass_frames;
                source_exact_bypass_frames += source_exact ? 1U : 0U;
                unavailable_or_occluded_source_exact +=
                    (packet_unavailable || packet_occluded) && source_exact ? 1U : 0U;
            }

            // Integrity scans and PPM output are deliberately outside this
            // component timing. Adapter plus worker/composite is the bounded
            // native work that would occur for a captured frame.
            frame_times.push_back(adapter_ms + worker_ms);
            write_ppm(output / "frames" / frame_name(output_index), pixels,
                      replay.width, replay.height, frame.lease.stride_bytes);
            events << "{\"outputFrame\":" << output_index
                   << ",\"sourceFrame\":" << source_index
                   << ",\"packetAvailable\":" << (packet_unavailable ? "false" : "true")
                   << ",\"packetOccluded\":" << (packet_occluded ? "true" : "false")
                   << ",\"residual\":" << (has_residual ? "true" : "false")
                   << ",\"sourceExact\":" << (source_exact ? "true" : "false")
                   << ",\"reason\":\"" << json_escape(reason)
                   << "\",\"viseme\":" << static_cast<unsigned int>(viseme)
                   << ",\"adapterMs\":" << adapter_ms
                   << ",\"workerAndCompositeMs\":" << worker_ms
                   << ",\"bounds\":[" << bounds.x << ',' << bounds.y << ','
                   << bounds.width << ',' << bounds.height << "]}\n";
            ++output_index;
        }

        const std::size_t required_unavailable_or_occluded_exact =
            unavailable_packets + occluded_packets;
        const bool integrity_passed = residual_frames > 0U &&
            bypass_frames == source_exact_bypass_frames &&
            unavailable_or_occluded_source_exact ==
                required_unavailable_or_occluded_exact &&
            outside_support_changed_pixels == 0U &&
            bilabial_residual_frames == exact_bilabial_contact_frames &&
            rate_limited_bypasses == 0U;

        std::ofstream report(output / "current-pixel-replay-report.json");
        if (!report) {
            throw std::runtime_error("could not create replay report");
        }
        report << std::fixed << std::setprecision(3)
               << "{\n  \"schema\":\"interactive-npcs-current-pixel-replay/v1\",\n"
               << "  \"status\":\"" << (integrity_passed ? "passed" : "failed") << "\",\n"
               << "  \"scope\":\"offline component fixture through native full-66 adapter, ReferenceMouthWorker, current-pixel compositor and CPU composition; not live capture, installed app, provider inference, audible playback, or game-load proof\",\n"
               << "  \"mouthAppearance\":\""
               << (source_only
                       ? "source-only current pixels; no character pack or invented oral texture"
                       : "identity-bound schema-4 oral-reference pack")
               << "\",\n"
               << "  \"identitySource\":\"manual reviewer-bound fixture; no face-recognition claim\",\n"
               << "  \"geometrySource\":\"exact current-frame full-66 packet with adapter smoothing alpha 1; no carried prior-frame geometry\",\n"
               << "  \"timeline\":\"native 15 Hz samples at source indices 0,2,4... from the exact 30 Hz source; no alternating 30 Hz residual/source artifact\",\n"
               << "  \"cueDelivery\":\"incremental single timed-viseme snapshot at the exact playback sample; no future or whole-utterance trajectory supplied to worker\",\n"
               << "  \"admittedSignalRateHz\":15,\n"
               << "  \"sourceInputFps\":30,\n"
               << "  \"outputFps\":15,\n"
               << "  \"sourceFrameStep\":2,\n"
               << "  \"sourceInputFrames\":" << frames.size() << ",\n"
               << "  \"outputFrames\":" << output_index << ",\n"
               << "  \"residualFrames\":" << residual_frames << ",\n"
               << "  \"bypassFrames\":" << bypass_frames << ",\n"
               << "  \"changedFrames\":" << changed_frames << ",\n"
               << "  \"sourceExactResidualFrames\":" << source_exact_residual_frames << ",\n"
               << "  \"sourceExactBypassFrames\":" << source_exact_bypass_frames << ",\n"
               << "  \"unavailablePackets\":" << unavailable_packets << ",\n"
               << "  \"occludedPackets\":" << occluded_packets << ",\n"
               << "  \"unavailableOrOccludedSourceExactFrames\":"
               << unavailable_or_occluded_source_exact << ",\n"
               << "  \"adapterBypasses\":" << adapter_bypasses << ",\n"
               << "  \"rateLimitedBypasses\":" << rate_limited_bypasses << ",\n"
               << "  \"workerBypasses\":" << worker_bypasses << ",\n"
               << "  \"bilabialResidualFrames\":" << bilabial_residual_frames << ",\n"
               << "  \"exactBilabialContactResidualFrames\":"
               << exact_bilabial_contact_frames << ",\n"
               << "  \"outsideResidualSupportChangedPixels\":"
               << outside_support_changed_pixels << ",\n"
               << "  \"adapterP50Ms\":" << percentile(adapter_times, 0.50) << ",\n"
               << "  \"adapterP95Ms\":" << percentile(adapter_times, 0.95) << ",\n"
               << "  \"workerAndCompositeP50Ms\":" << percentile(worker_times, 0.50) << ",\n"
               << "  \"workerAndCompositeP95Ms\":" << percentile(worker_times, 0.95) << ",\n"
               << "  \"wholeFrameComponentP95Ms\":" << percentile(frame_times, 0.95) << ",\n"
               << "  \"timingExcludes\":[\"PPM write\",\"landmark inference\",\"audio decode\",\"capture\",\"presentation\",\"game load\"],\n"
               << "  \"audioSha256\":\"" << audio_digest << "\",\n"
               << "  \"cueSha256\":\"" << sha256_bytes(read_binary(cue_path)) << "\",\n"
               << "  \"landmarkReplaySha256\":\""
               << sha256_bytes(read_binary(landmark_path)) << "\"\n}\n";

        std::cout << "source_frames=" << frames.size()
                  << " output_frames=" << output_index
                  << " residuals=" << residual_frames
                  << " bypasses=" << bypass_frames
                  << " worker_p95_ms=" << percentile(worker_times, 0.95)
                  << " integrity=" << (integrity_passed ? "passed" : "failed") << '\n';
        return integrity_passed ? 0 : 2;
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << '\n';
        return 1;
    }
}
