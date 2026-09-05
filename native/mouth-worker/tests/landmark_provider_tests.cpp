#include "npc/mouth_worker/landmark_provider.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <limits>
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

AdmittedLandmarkProviderLaunchV1 yunet_launch() {
    auto value = launch();
    value.pack_id = std::string(admitted_yunet_openseeface_pack_id_v1);
    value.detector_model = value.artifact_root / "models" /
                           "face_detection_yunet_2023mar.onnx";
    value.detector_size_bytes = 232'589U;
    value.detector_sha256 = std::string(admitted_yunet_detector_sha256_v1);
    value.landmark_sha256 = std::string(admitted_openseeface_lm1_sha256_v1);
    return value;
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

    const auto yunet = yunet_launch();
    require(validate_landmark_provider_launch_v1(yunet),
            "exact pinned YuNet640 plus LM1 alternate pack admitted");
    auto changed_yunet = yunet;
    changed_yunet.detector_sha256[0] = '0';
    require(!validate_landmark_provider_launch_v1(changed_yunet),
            "changed YuNet detector digest rejected");
    changed_yunet = yunet;
    changed_yunet.landmark_sha256[0] = '0';
    require(!validate_landmark_provider_launch_v1(changed_yunet),
            "changed LM1 digest rejected for YuNet pack");
}

void test_yunet_decoder_matches_stride_geometry_and_seed_identity() {
    constexpr float log_half = -0.6931471805599453F;
    const std::array<float, 2U> classes{0.99F, 0.81F};
    const std::array<float, 2U> objects{1.0F, 1.0F};
    const std::array<float, 8U> boxes{
        0.5F, 0.5F, log_half, log_half,
        0.5F, 0.5F, log_half, log_half,
    };
    const YuNetDetectorLevelV1 level{8U, 2U, 1U, classes, objects, boxes};
    const auto selected = decode_and_select_yunet_face_v1(
        std::span<const YuNetDetectorLevelV1>(&level, 1U),
        16U, 8U, 16U, 8U, 160U, 80U,
        NormalizedRect{0.625, 0.10, 0.25, 0.80});
    require(selected.has_value(), "YuNet stride output decoded");
    require(std::abs(selected->bounds.x - 0.625) < 1.0e-6 &&
                std::abs(selected->bounds.y - 0.25) < 1.0e-6 &&
                std::abs(selected->bounds.width - 0.25) < 1.0e-6 &&
                std::abs(selected->bounds.height - 0.50) < 1.0e-6,
            "YuNet center/size exponent decode matches OpenCV geometry");
    require(std::abs(selected->confidence - 0.9) < 1.0e-6,
            "YuNet confidence is geometric mean of class and object outputs");
    require(selected->confidence < 0.99,
            "seed-overlap identity selection rejects higher-score different face");

    const auto padded = decode_and_select_yunet_face_v1(
        std::span<const YuNetDetectorLevelV1>(&level, 1U),
        16U, 8U, 8U, 8U, 80U, 80U,
        NormalizedRect{0.10, 0.10, 0.50, 0.80});
    require(padded.has_value() && padded->bounds.x < 0.75,
            "detections centered in padded tensor columns are excluded");

    constexpr std::size_t crowded_cells = 2'048U;
    std::vector<float> crowded_classes(crowded_cells);
    std::vector<float> crowded_objects(crowded_cells, 1.0F);
    std::vector<float> crowded_boxes(crowded_cells * 4U, 0.0F);
    for (std::size_t index = 0U; index < crowded_cells; ++index) {
        crowded_classes[index] = 0.51F + static_cast<float>(index) /
            static_cast<float>(crowded_cells) * 0.48F;
    }
    const YuNetDetectorLevelV1 crowded_level{
        8U, 64U, 32U, crowded_classes, crowded_objects, crowded_boxes};
    const auto crowded = decode_and_select_yunet_face_v1(
        std::span<const YuNetDetectorLevelV1>(&crowded_level, 1U),
        512U, 256U, 512U, 256U, 512U, 256U,
        NormalizedRect{0.0, 0.0, 1.0, 1.0});
    require(crowded.has_value() && crowded->confidence > 0.99,
            "pre-NMS candidate bound retains the highest-confidence eligible face");
}

void test_yunet_tracking_periodic_refresh_loss_and_actor_lock() {
    const YuNetTrackingPolicyV1 policy{12U};
    require(validate_yunet_tracking_policy_v1(policy),
            "bounded YuNet detector refresh policy accepted");
    require(!validate_yunet_tracking_policy_v1(YuNetTrackingPolicyV1{9U}) &&
                !validate_yunet_tracking_policy_v1(YuNetTrackingPolicyV1{16U}),
            "unmeasured detector refresh periods rejected");
    require(choose_yunet_tracking_action_v1(false, 0U, policy) ==
                YuNetTrackingActionV1::reacquire_face,
            "lost track reacquires instead of borrowing another ROI");
    require(choose_yunet_tracking_action_v1(true, 11U, policy) ==
                YuNetTrackingActionV1::track_landmarks &&
                choose_yunet_tracking_action_v1(true, 12U, policy) ==
                YuNetTrackingActionV1::reacquire_face,
            "detector refresh fires at configured bounded cadence");

    const NormalizedRect locked{0.32, 0.16, 0.16, 0.28};
    require(yunet_face_matches_locked_actor_v1(
                NormalizedRect{0.33, 0.17, 0.16, 0.28}, locked),
            "nearby periodic detection preserves actor lock");
    require(!yunet_face_matches_locked_actor_v1(
                NormalizedRect{0.67, 0.16, 0.16, 0.28}, locked),
            "higher-confidence different actor cannot replace locked actor");

    std::array<NormalizedLandmark, openseeface_landmark_count_v1> before{};
    std::array<NormalizedLandmark, openseeface_landmark_count_v1> after{};
    for (std::size_t index = 0U; index < before.size(); ++index) {
        const double x = 0.35 + static_cast<double>(index % 8U) * 0.012;
        const double y = 0.21 + static_cast<double>(index / 8U) * 0.018;
        before[index] = {x, y, 0.95};
        after[index] = {0.405 + (x - 0.395) * 1.02,
                        0.285 + (y - 0.275) * 1.02, 0.94};
    }
    const auto updated = update_yunet_tracked_face_v1(locked, before, after);
    require(updated.has_value() && updated->center_motion_face_fraction < 0.10 &&
                std::abs(updated->scale_ratio - 1.02) < 1.0e-6,
            "small landmark motion advances the locked ROI without detector inference");

    auto jumped = after;
    for (auto& point : jumped) point.x += 0.25;
    require(!update_yunet_tracked_face_v1(locked, before, jumped),
            "high-motion landmark drift triggers detector reacquisition");
    auto lost = after;
    for (std::size_t index = 0U; index < 30U; ++index) lost[index].confidence = 0.20;
    require(!update_yunet_tracked_face_v1(locked, before, lost),
            "insufficient current landmark coverage is treated as lost tracking");

    auto partially_invalid = after;
    for (std::size_t index = 24U; index < 48U; ++index) {
        partially_invalid[index].confidence =
            std::numeric_limits<double>::quiet_NaN();
        partially_invalid[index].x += 0.40;
        partially_invalid[index].y += 0.40;
    }
    require(update_yunet_tracked_face_v1(locked, before, partially_invalid).has_value(),
            "nonfinite-confidence landmarks are excluded from both geometry passes");
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
    test_yunet_decoder_matches_stride_geometry_and_seed_identity();
    test_yunet_tracking_periodic_refresh_loss_and_actor_lock();
    test_queue_one_exact_packet_binding_cancel_and_unload();
    test_stale_and_wrong_binding_fail_open();
    std::cout << "PASS: admitted landmark provider boundary, queue-one, exact binding, cancel/unload\n";
    return 0;
}
