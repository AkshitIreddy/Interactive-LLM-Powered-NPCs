// Offline component comparison: externally measured, frame-indexed geometry
// drives the real adapter and worker. This is not an admitted product provider.
#include "headless_proof_support.hpp"

namespace {
struct ReplayPackets {
    std::uint32_t width{}, height{}, fps{};
    std::vector<std::optional<OpenSeeFaceLandmarkPacketV1>> packets;
};

ReplayPackets read_packets(const std::filesystem::path& path) {
    std::ifstream input(path);
    std::string magic, line;
    std::size_t count{};
    ReplayPackets result;
    if (!(input >> magic >> result.width >> result.height >> count >> result.fps) ||
        magic != "npc-landmark-replay-v1" || result.fps != 30U ||
        count == 0U || count > 1800U || result.width == 0U || result.width > 4096U ||
        result.height == 0U || result.height > 4096U) {
        throw std::runtime_error("invalid external landmark replay header");
    }
    std::getline(input, line);
    if (line.find_first_not_of(" \t\r") != std::string::npos)
        throw std::runtime_error("extra replay header fields");
    for (std::size_t expected = 0U; expected < count; ++expected) {
        if (!std::getline(input, line)) throw std::runtime_error("truncated landmark replay");
        std::istringstream row(line);
        std::size_t index{};
        unsigned valid{}, occluded{};
        if (!(row >> index >> valid) || index != expected || valid > 1U)
            throw std::runtime_error("landmark replay indices must be contiguous");
        OpenSeeFaceLandmarkPacketV1 packet{};
        if (valid != 0U) {
            if (!(row >> packet.detector_confidence >> packet.landmark_confidence >>
                packet.visibility_ratio >> packet.pose.yaw >> packet.pose.pitch >>
                packet.pose.roll >> packet.face_bounds.x >> packet.face_bounds.y >>
                packet.face_bounds.width >> packet.face_bounds.height >> occluded) || occluded > 1U)
                throw std::runtime_error("invalid landmark replay geometry");
            packet.mouth_occluded = occluded != 0U;
            for (auto& point : packet.landmarks) {
                if (!(row >> point.x >> point.y >> point.confidence))
                    throw std::runtime_error("landmark replay requires 66 complete points");
            }
        }
        std::string extra;
        if (row >> extra) throw std::runtime_error("extra landmark replay fields");
        result.packets.push_back(valid ? std::optional(packet) : std::nullopt);
    }
    while (std::getline(input, line)) {
        if (line.find_first_not_of(" \t\r") != std::string::npos)
            throw std::runtime_error("extra landmark replay rows");
    }
    return result;
}
}

int main(int argc, char** argv) {
    try {
        const bool unsmoothed_geometry = argc == 8 &&
            std::string_view(argv[7]) == "--unsmoothed-geometry";
        if (argc != 7 && !unsmoothed_geometry) throw std::runtime_error(
            "usage: replay_tracked_mouth <frames> <audio.wav> <atlas> <cues.tsv> <landmarks.tsv> <fresh-output> [--unsmoothed-geometry]");
        auto frames = read_source_frames(argv[1]);
        const auto audio = read_wav_pcm16(argv[2]);
        const auto packets = read_packets(argv[5]);
        const std::filesystem::path output(argv[6]);
        if (std::filesystem::exists(output)) throw std::runtime_error("output already exists");
        const auto samples = audio.samples.size() / audio.channels;
        const auto count = (samples * 30U + audio.sample_rate - 1U) / audio.sample_rate;
        if (count != frames.size() || count != packets.packets.size() ||
            frames.front().lease.width != packets.width || frames.front().lease.height != packets.height)
            throw std::runtime_error("audio, frames, and geometry must cover the same complete sequence");
        std::ifstream cue_stream(argv[4]);
        const auto cues = read_review_cues(cue_stream, audio.sample_rate, samples,
                                           sha256_bytes(read_binary(argv[2])));
        constexpr std::uint64_t generation = 1U;
        const TrackBinding track{generation, 0x50455045U, 0x43503230U, 1U};
        ReferenceMouthWorker worker(generation);
        if (!worker.install_atlas(read_review_atlas(argv[3], generation, track)))
            throw std::runtime_error("atlas failed native admission");
        OpenSeeFaceAdapterPolicy adapter_policy{};
        if (unsmoothed_geometry) adapter_policy.smoothing_alpha = 1.0;
        OpenSeeFaceSignalAdapter adapter(generation, adapter_policy);
        AppearanceGateEvidenceV1 appearance{};
        // The reviewer selects one character in this recorded sequence.
        // These are a manual identity fixture, not face-recognition scores.
        appearance.runtime_actor_id = track.actor_id;
        appearance.descriptor_revision = 1U;
        appearance.expected_descriptor_digest_high = appearance.observed_descriptor_digest_high = 1U;
        appearance.expected_descriptor_digest_low = appearance.observed_descriptor_digest_low = 2U;
        appearance.similarity = appearance.temporal_iou = 1.0;
        appearance.identity_locked = appearance.target_visible = true;
        VisualResourceStateV1 resources{};
        resources.admitted_signal_rate_hz = 15U;
        std::optional<TrackingEvidence> carried;
        std::string last_sample_reason = "no-prior-sample";
        std::vector<double> render_times;
        std::vector<double> frame_process_times;
        std::size_t rendered{}, unavailable{}, adapter_bypasses{}, worker_bypasses{};
        std::filesystem::create_directories(output / "frames");
        std::ofstream events(output / "frames.jsonl");
        for (std::size_t index = 0U; index < count; ++index) {
            const auto frame_started = std::chrono::steady_clock::now();
            auto& frame = frames[index];
            const auto at = 10'000'000'000LL + static_cast<Nanoseconds>(index) * 1'000'000'000LL / 30;
            frame.identity = {index + 1000U, 1U, 1U, at};
            frame.lease.lease_nonce_low = index + 1U;
            frame.lease.expires_at_ns = at + 500'000'000LL;
            std::string reason = carried ? "carried-valid-geometry" :
                "no-carried-geometry:" + last_sample_reason;
            if (index % 2U == 0U) {
                carried.reset();
                if (packets.packets[index]) {
                    auto packet = *packets.packets[index];
                    packet.provider_instance_id = 0x59554e4554ULL;
                    packet.track = track;
                    packet.frame = frame.identity;
                    packet.source_frame_qpc = static_cast<std::uint64_t>(at);
                    packet.qpc_frequency = 1'000'000'000U;
                    packet.measured_at_ns = at;
                    const auto decision = adapter.adapt(packet, appearance, resources, frame.identity, at + 1'000'000LL);
                    reason = std::string(to_string(decision.disposition));
                    if (decision.accepted()) carried = decision.tracking;
                    else ++adapter_bypasses;
                } else {
                    ++unavailable;
                    reason = "external-detector-or-landmark-unavailable";
                }
                last_sample_reason = reason;
            }
            auto pixels = frame.bgra;
            bool has_residual = false;
            NormalizedRect bounds{};
            if (carried) {
                auto tracking = *carried;
                tracking.frame = frame.identity;
                // Preserve measurement time on carried frames; never claim a
                // new model observation when using the previous 15Hz sample.
                const auto first = index * audio.sample_rate / 30U;
                const auto last = std::min(samples, (index + 1U) * audio.sample_rate / 30U);
                MouthDrive drive{};
                drive.kind = DriveKind::timed_viseme;
                drive.clock = {generation, 1U, first, last - first, first, audio.sample_rate, audio.channels, at};
                drive.viseme = review_viseme_at(cues, first);
                if (!worker.submit({track, frame, tracking, std::move(drive), at + 100'000'000LL}))
                    throw std::runtime_error("worker rejected replay submission");
                const auto started = std::chrono::steady_clock::now();
                auto result = worker.process_latest(frame.identity, at + 2'000'000LL);
                render_times.push_back(std::chrono::duration<double, std::milli>(
                    std::chrono::steady_clock::now() - started).count());
                if (result.has_residual()) {
                    pixels = composite_over_source(frame, result.residual);
                    bounds = result.residual.normalized_bounds;
                    has_residual = true;
                    ++rendered;
                } else {
                    reason = std::string(to_string(result.disposition));
                    ++worker_bypasses;
                }
            }
            frame_process_times.push_back(std::chrono::duration<double, std::milli>(
                std::chrono::steady_clock::now() - frame_started).count());
            std::ostringstream name;
            name << "frame-" << std::setfill('0') << std::setw(5) << index << ".ppm";
            write_ppm(output / "frames" / name.str(), pixels, packets.width, packets.height,
                      frame.lease.stride_bytes);
            events << "{\"frame\":" << index << ",\"residual\":" << (has_residual ? "true" : "false")
                   << ",\"reason\":\"" << reason << "\",\"bounds\":[" << bounds.x << ',' << bounds.y
                   << ',' << bounds.width << ',' << bounds.height << "]}\n";
        }
        std::sort(render_times.begin(), render_times.end());
        const auto p95 = render_times.empty() ? 0.0 : render_times[
            std::min(render_times.size() - 1U, static_cast<std::size_t>(std::ceil(render_times.size() * .95)) - 1U)];
        std::sort(frame_process_times.begin(), frame_process_times.end());
        const auto frame_p95 = frame_process_times.empty() ? 0.0 : frame_process_times[
            std::min(frame_process_times.size() - 1U,
                     static_cast<std::size_t>(std::ceil(frame_process_times.size() * .95)) - 1U)];
        std::ofstream report(output / "replay-report.json");
        report << "{\n  \"scope\":\"external-model-packet replay through native adapter and compositor; not installed-provider or live-game proof\",\n"
               << "  \"identitySource\":\"manual reviewer selection; no recognition claim\",\n"
               << "  \"sourceFrames\":" << count << ",\n  \"residualFrames\":" << rendered
               << ",\n  \"detectorUnavailableSamples\":" << unavailable
               << ",\n  \"adapterBypasses\":" << adapter_bypasses
               << ",\n  \"workerBypasses\":" << worker_bypasses
               << ",\n  \"workerP95Ms\":" << p95
               << ",\n  \"geometrySmoothingAlpha\":" << adapter_policy.smoothing_alpha
               << ",\n  \"nativeFrameProcessP95Ms\":" << frame_p95
               << ",\n  \"nativeFrameProcessScope\":\"adapter, worker and composition including source copy; excludes model inference, frame IO, capture and presentation\""
               << ",\n  \"landmarkReplaySha256\":\"" << sha256_bytes(read_binary(argv[5]))
               << "\",\n  \"mouthCueSha256\":\"" << sha256_bytes(read_binary(argv[4])) << "\"\n}\n";
        std::cout << "frames=" << count << " residuals=" << rendered << " worker_p95_ms=" << p95 << '\n';
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << '\n';
        return 1;
    }
}
