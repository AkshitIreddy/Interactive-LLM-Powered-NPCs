#include "npc/mouth_worker/landmark_provider.hpp"

#include <algorithm>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <memory>
#include <string>
#include <utility>

using namespace npc::mouth;

namespace {

void require(const bool condition, const char* message) {
    if (!condition) {
        std::cerr << "FAIL: " << message << '\n';
        std::exit(1);
    }
}

class FakeProvider final : public NativeLandmarkProviderV1 {
public:
    bool load(const AdmittedLandmarkProviderLaunchV1&,
              const std::uint64_t generation,
              std::string&) override {
        loaded_ = load_succeeds;
        generation_ = loaded_ ? generation : 0U;
        return loaded_;
    }

    std::optional<OpenSeeFaceLandmarkPacketV1> infer(
        const LandmarkInferenceWorkV1& work,
        const Nanoseconds now_ns,
        std::string& failure) override {
        ++infer_calls;
        if (!loaded_ || infer_fails) {
            failure = "fake_provider_failure";
            return std::nullopt;
        }
        OpenSeeFaceLandmarkPacketV1 packet{};
        packet.provider_instance_id = 0x46414b4550524f56ULL;
        packet.track = work.track;
        packet.frame = work.frame;
        packet.source_frame_qpc = work.source_frame_qpc;
        packet.qpc_frequency = work.qpc_frequency;
        packet.face_bounds = work.seed_face_bounds;
        for (std::size_t index = 0U; index < packet.landmarks.size(); ++index) {
            const double offset = static_cast<double>(index % 6U) * 0.004;
            packet.landmarks[index] = {0.42 + offset, 0.45 + offset, 0.99};
        }
        packet.detector_confidence = 0.98;
        packet.landmark_confidence = 0.97;
        packet.visibility_ratio = 0.96;
        packet.measured_at_ns =
            std::max(now_ns + completion_offset_ns, work.frame.captured_at_ns);
        if (return_wrong_frame) ++packet.frame.sequence;
        return packet;
    }

    bool cancel_to(const std::uint64_t generation) noexcept override {
        if (!loaded_ || generation <= generation_) return false;
        generation_ = generation;
        return true;
    }

    void unload() noexcept override {
        loaded_ = false;
        generation_ = 0U;
        ++unload_calls;
    }

    bool loaded() const noexcept override { return loaded_; }

    bool load_succeeds{true};
    bool infer_fails{};
    bool return_wrong_frame{};
    Nanoseconds completion_offset_ns{};
    std::uint64_t infer_calls{};
    std::uint64_t unload_calls{};

private:
    bool loaded_{};
    std::uint64_t generation_{};
};

AdmittedLandmarkProviderLaunchV1 launch() {
    const auto root = std::filesystem::absolute("fixture-openseeface-pack");
    return {
        1U,
        std::string(admitted_openseeface_pack_id_v1),
        std::string(admitted_openseeface_revision_v1),
        root,
        root / "models" / "mnv3_detection_opt.onnx",
        root / "models" / "lm_model1_opt.onnx",
        root / "runtime" / "lib" / "onnxruntime.dll",
        root / "runtime" / "lib" / "onnxruntime_providers_shared.dll",
        568'302U,
        4'842'329U,
        14'854'688U,
        19'456U,
        std::string(64U, 'a'),
        std::string(64U, 'b'),
        std::string(64U, 'c'),
        std::string(64U, 'e'),
        std::string(64U, 'd'),
        std::string(admitted_openseeface_runtime_revision_v1),
        std::string(admitted_openseeface_backend_v1),
        15U,
        1U,
        42U,
    };
}

LandmarkInferenceWorkV1 work(const std::uint64_t frame_sequence,
                             const Nanoseconds deadline = 1'000'000) {
    LandmarkInferenceWorkV1 value{};
    value.track = {7U, 55U, 66U, 3U};
    value.frame = {frame_sequence, 4U, 5U, 1000 + static_cast<Nanoseconds>(frame_sequence)};
    value.source_frame_qpc = 10'000U + frame_sequence;
    value.qpc_frequency = 10'000'000U;
    value.seed_face_bounds = {0.25, 0.20, 0.50, 0.65};
    value.source.identity = value.frame;
    value.source.lease.schema_version = 1U;
    value.source.lease.transport = LeaseTransport::cpu_reference;
    value.source.lease.width = 16U;
    value.source.lease.height = 16U;
    value.source.lease.stride_bytes = 64U;
    value.source.lease.expires_at_ns = deadline;
    value.source.bgra.resize(1024U, 128U);
    value.deadline_ns = deadline;
    return value;
}

void test_launch_is_exact_and_fail_closed() {
    auto provider = std::make_unique<FakeProvider>();
    auto* fake = provider.get();
    NativeLandmarkCoordinatorV1 coordinator(std::move(provider));
    auto invalid = launch();
    invalid.artifact_root = "relative-pack";
    require(coordinator.load(invalid, 7U, 10).disposition ==
                LandmarkProviderDispositionV1::bypass_invalid,
            "relative artifact root rejected");
    require(!fake->loaded(), "invalid launch did not load provider");
    auto exact = launch();
    exact.backend = "gpu";
    require(!validate_landmark_provider_launch_v1(exact), "wrong backend rejected");
}

void test_queue_one_exact_packet_binding_cancel_and_unload() {
    auto provider = std::make_unique<FakeProvider>();
    auto* fake = provider.get();
    NativeLandmarkCoordinatorV1 coordinator(std::move(provider));
    require(coordinator.load(launch(), 7U, 10).disposition ==
                LandmarkProviderDispositionV1::ready,
            "admitted provider loaded");
    require(coordinator.submit(work(1U), 20).disposition ==
                LandmarkProviderDispositionV1::ready,
            "first work queued");
    const auto replacement = coordinator.submit(work(2U), 21);
    require(replacement.disposition ==
                LandmarkProviderDispositionV1::replaced_before_processing,
            "newest work replaces older queue item");
    require(replacement.frame.sequence == 1U && coordinator.queue_replacements() == 1U,
            "replacement receipt identifies dropped exact frame");
    const auto produced = coordinator.process_latest(30);
    require(produced.produced_packet() && produced.completed_work.has_value() &&
                produced.completed_work->source.identity == work(2U).frame &&
                produced.frame.sequence == 2U,
            "latest exact frame produced packet and retained that same source frame");
    require(produced.packet->track == work(2U).track &&
                produced.packet->frame == work(2U).frame &&
                produced.packet->source_frame_qpc == work(2U).source_frame_qpc,
            "packet retains actor track frame and qpc binding");
    require(fake->infer_calls == 1U, "replaced frame never reached inference");

    fake->completion_offset_ns = 7;
    require(coordinator.submit(work(4U), 31).disposition ==
                LandmarkProviderDispositionV1::ready,
            "provider completion timing candidate queued");
    const auto completed_after_infer_start = coordinator.process_latest(32);
    require(completed_after_infer_start.produced_packet() &&
                completed_after_infer_start.completed_at_ns == work(4U).frame.captured_at_ns,
            "provider completion may follow inference start when still before deadline");
    fake->completion_offset_ns = 0;

    require(coordinator.submit(work(3U), 40).disposition ==
                LandmarkProviderDispositionV1::ready,
            "cancel candidate queued");
    const auto cancelled = coordinator.cancel_to(8U, 41);
    require(cancelled.disposition == LandmarkProviderDispositionV1::bypass_cancelled &&
                cancelled.frame.sequence == 3U && coordinator.active_generation() == 8U,
            "cancel advances provider generation and drops exact work");
    require(coordinator.process_latest(42).disposition ==
                LandmarkProviderDispositionV1::bypass_invalid,
            "cancelled queue stays empty");
    require(coordinator.unload(43).disposition == LandmarkProviderDispositionV1::unloaded &&
                coordinator.active_generation() == 0U && !fake->loaded(),
            "unload releases provider state");
}

void test_stale_and_wrong_binding_fail_open() {
    auto provider = std::make_unique<FakeProvider>();
    auto* fake = provider.get();
    NativeLandmarkCoordinatorV1 coordinator(std::move(provider));
    require(coordinator.load(launch(), 7U, 10).disposition ==
                LandmarkProviderDispositionV1::ready,
            "provider loaded");
    require(coordinator.submit(work(1U, 25), 20).disposition ==
                LandmarkProviderDispositionV1::ready,
            "short deadline queued");
    require(coordinator.process_latest(26).disposition ==
                LandmarkProviderDispositionV1::bypass_stale && fake->infer_calls == 0U,
            "expired frame dropped before inference");

    fake->return_wrong_frame = true;
    require(coordinator.submit(work(2U), 30).disposition ==
                LandmarkProviderDispositionV1::ready,
            "wrong-frame fake queued");
    const auto wrong = coordinator.process_latest(31);
    require(wrong.disposition == LandmarkProviderDispositionV1::bypass_provider_failure &&
                !wrong.packet,
            "provider cannot escape exact frame binding");
}

} // namespace

int main() {
    test_launch_is_exact_and_fail_closed();
    test_queue_one_exact_packet_binding_cancel_and_unload();
    test_stale_and_wrong_binding_fail_open();
    std::cout << "PASS: admitted landmark provider boundary, queue-one, exact binding, cancel/unload\n";
    return 0;
}
