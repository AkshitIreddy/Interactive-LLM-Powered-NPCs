#include "npc/mouth_worker/source_frame_guard.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <optional>
#include <utility>

namespace npc::mouth {
namespace {

constexpr std::size_t patch_columns = 48U;
constexpr std::size_t patch_rows = 32U;
constexpr std::size_t patch_samples = patch_columns * patch_rows;
constexpr std::size_t luma_bins = 16U;
constexpr std::size_t chroma_bins = 12U;
constexpr std::size_t gradient_bins = 12U;
constexpr std::size_t histogram_values =
    luma_bins + chroma_bins + chroma_bins + gradient_bins;
constexpr std::size_t spatial_cells = 6U;
constexpr std::size_t spatial_channels = 4U;
constexpr std::size_t descriptor_values =
    histogram_values + spatial_cells * spatial_channels;
constexpr double epsilon = 1.0e-9;

struct Sample {
  double y{};
  double cr{};
  double cb{};
  bool valid{};
};

struct SampledPatch {
  std::array<Sample, patch_samples> samples{};
  std::array<double, patch_samples> u{};
  std::array<double, patch_samples> v{};
  std::size_t valid_samples{};
};

struct Descriptor {
  std::array<double, descriptor_values> values{};
  double valid_sample_ratio{};
};

struct Innovation {
  double luma_histogram{};
  double cr_histogram{};
  double cb_histogram{};
  double gradient_histogram{};
  double spatial_color{};
  double spatial_texture{};
};

[[nodiscard]] bool finite_unit(const double value) noexcept {
  return std::isfinite(value) && value >= 0.0 && value <= 1.0;
}

[[nodiscard]] bool valid_rect(const NormalizedRect &value) noexcept {
  return std::isfinite(value.x) && std::isfinite(value.y) &&
         std::isfinite(value.width) && std::isfinite(value.height) &&
         value.x >= 0.0 && value.y >= 0.0 && value.width > 0.0 &&
         value.height > 0.0 && value.right() <= 1.0 && value.bottom() <= 1.0;
}

[[nodiscard]] bool valid_landmark(const NormalizedLandmark &value) noexcept {
  return std::isfinite(value.x) && std::isfinite(value.y) &&
         finite_unit(value.confidence) && value.x >= 0.0 && value.x <= 1.0 &&
         value.y >= 0.0 && value.y <= 1.0;
}

[[nodiscard]] bool valid_frame(const CpuFrame &source) noexcept {
  if (source.lease.transport != LeaseTransport::cpu_reference ||
      source.lease.format != PixelFormat::bgra8_unorm_premultiplied ||
      source.lease.width == 0U || source.lease.height == 0U) {
    return false;
  }
  const auto minimum_stride =
      static_cast<std::uint64_t>(source.lease.width) * 4U;
  const auto required_bytes =
      static_cast<std::uint64_t>(source.lease.stride_bytes) *
      source.lease.height;
  return source.lease.stride_bytes >= minimum_stride &&
         required_bytes <= source.bgra.size();
}

[[nodiscard]] bool valid_tracking(const TrackingEvidence &tracking) noexcept {
  if (tracking.track.actor_id == 0U || tracking.track.track_id == 0U ||
      tracking.track.track_epoch == 0U || tracking.frame.sequence == 0U ||
      tracking.frame.geometry_epoch == 0U ||
      !valid_rect(tracking.face_bounds) ||
      tracking.mouth_landmarks.contour_points == 0U ||
      tracking.mouth_landmarks.contour_points >
          tracking.mouth_landmarks.contour.size() ||
      !valid_landmark(tracking.mouth_landmarks.left_corner) ||
      !valid_landmark(tracking.mouth_landmarks.right_corner)) {
    return false;
  }
  for (std::size_t index = 0U; index < tracking.mouth_landmarks.contour_points;
       ++index) {
    if (!valid_landmark(tracking.mouth_landmarks.contour[index])) {
      return false;
    }
  }
  return true;
}

[[nodiscard]] Sample pixel_at(const CpuFrame &frame, const double x,
                              const double y) noexcept {
  const auto width = static_cast<std::int64_t>(frame.lease.width);
  const auto height = static_cast<std::int64_t>(frame.lease.height);
  if (!std::isfinite(x) || !std::isfinite(y) || x < 0.0 || y < 0.0 ||
      x > static_cast<double>(width - 1) ||
      y > static_cast<double>(height - 1)) {
    return {};
  }

  const auto x0 = static_cast<std::int64_t>(std::floor(x));
  const auto y0 = static_cast<std::int64_t>(std::floor(y));
  const auto x1 = std::min(x0 + 1, width - 1);
  const auto y1 = std::min(y0 + 1, height - 1);
  const double tx = x - static_cast<double>(x0);
  const double ty = y - static_cast<double>(y0);

  const auto channel = [&](const std::int64_t px, const std::int64_t py,
                           const std::size_t offset) noexcept {
    const auto index = static_cast<std::size_t>(py) * frame.lease.stride_bytes +
                       static_cast<std::size_t>(px) * 4U;
    const double alpha = static_cast<double>(frame.bgra[index + 3U]);
    const double encoded = static_cast<double>(frame.bgra[index + offset]);
    if (alpha <= 0.0) {
      return 0.0;
    }
    return std::clamp(encoded * 255.0 / alpha, 0.0, 255.0);
  };
  const auto interpolate = [&](const std::size_t offset) noexcept {
    const double top =
        channel(x0, y0, offset) * (1.0 - tx) + channel(x1, y0, offset) * tx;
    const double bottom =
        channel(x0, y1, offset) * (1.0 - tx) + channel(x1, y1, offset) * tx;
    return top * (1.0 - ty) + bottom * ty;
  };

  const double blue = interpolate(0U);
  const double green = interpolate(1U);
  const double red = interpolate(2U);
  const double luminance = 0.299 * red + 0.587 * green + 0.114 * blue;
  return {
      luminance,
      std::clamp(128.0 + 0.500 * red - 0.419 * green - 0.081 * blue, 0.0,
                 255.0),
      std::clamp(128.0 - 0.169 * red - 0.331 * green + 0.500 * blue, 0.0,
                 255.0),
      true,
  };
}

[[nodiscard]] SampledPatch
sample_mouth_patch(const CpuFrame &source,
                   const TrackingEvidence &tracking) noexcept {
  SampledPatch patch{};
  const double frame_width = static_cast<double>(source.lease.width);
  const double frame_height = static_cast<double>(source.lease.height);
  const auto left = tracking.mouth_landmarks.left_corner;
  const auto right = tracking.mouth_landmarks.right_corner;
  const double left_x = left.x * frame_width;
  const double left_y = left.y * frame_height;
  const double right_x = right.x * frame_width;
  const double right_y = right.y * frame_height;
  const double axis_x = right_x - left_x;
  const double axis_y = right_y - left_y;
  const double mouth_width = std::hypot(axis_x, axis_y);
  if (!(mouth_width >= 4.0) || !std::isfinite(mouth_width)) {
    return patch;
  }
  const double unit_x = axis_x / mouth_width;
  const double unit_y = axis_y / mouth_width;
  const double perpendicular_x = -unit_y;
  const double perpendicular_y = unit_x;

  double center_x = 0.0;
  double center_y = 0.0;
  const auto count =
      static_cast<std::size_t>(tracking.mouth_landmarks.contour_points);
  for (std::size_t index = 0U; index < count; ++index) {
    center_x += tracking.mouth_landmarks.contour[index].x * frame_width;
    center_y += tracking.mouth_landmarks.contour[index].y * frame_height;
  }
  center_x /= static_cast<double>(count);
  center_y /= static_cast<double>(count);

  for (std::size_t row = 0U; row < patch_rows; ++row) {
    const double v = -0.62 + 1.32 * static_cast<double>(row) /
                                 static_cast<double>(patch_rows - 1U);
    for (std::size_t column = 0U; column < patch_columns; ++column) {
      const double u = -0.95 + 1.90 * static_cast<double>(column) /
                                   static_cast<double>(patch_columns - 1U);
      const auto index = row * patch_columns + column;
      patch.u[index] = u;
      patch.v[index] = v;
      patch.samples[index] = pixel_at(
          source, center_x + (unit_x * u + perpendicular_x * v) * mouth_width,
          center_y + (unit_y * u + perpendicular_y * v) * mouth_width);
      patch.valid_samples += patch.samples[index].valid ? 1U : 0U;
    }
  }
  return patch;
}

[[nodiscard]] SampledPatch
sample_face_patch(const CpuFrame &source,
                  const TrackingEvidence &tracking) noexcept {
  SampledPatch patch{};
  const double frame_width = static_cast<double>(source.lease.width);
  const double frame_height = static_cast<double>(source.lease.height);
  for (std::size_t row = 0U; row < patch_rows; ++row) {
    const double v =
        static_cast<double>(row) / static_cast<double>(patch_rows - 1U);
    for (std::size_t column = 0U; column < patch_columns; ++column) {
      const double u =
          static_cast<double>(column) / static_cast<double>(patch_columns - 1U);
      const auto index = row * patch_columns + column;
      patch.u[index] = u;
      patch.v[index] = v;
      patch.samples[index] =
          pixel_at(source,
                   (tracking.face_bounds.x + tracking.face_bounds.width * u) *
                       frame_width,
                   (tracking.face_bounds.y + tracking.face_bounds.height * v) *
                       frame_height);
      patch.valid_samples += patch.samples[index].valid ? 1U : 0U;
    }
  }
  return patch;
}

[[nodiscard]] std::array<double, patch_samples>
gradients(const SampledPatch &patch) noexcept {
  std::array<double, patch_samples> result{};
  const auto value = [&](const std::size_t row,
                         const std::size_t column) noexcept {
    const auto &sample = patch.samples[row * patch_columns + column];
    return sample.valid ? sample.y : 0.0;
  };
  for (std::size_t row = 1U; row + 1U < patch_rows; ++row) {
    for (std::size_t column = 1U; column + 1U < patch_columns; ++column) {
      const double gx =
          -value(row - 1U, column - 1U) + value(row - 1U, column + 1U) -
          2.0 * value(row, column - 1U) + 2.0 * value(row, column + 1U) -
          value(row + 1U, column - 1U) + value(row + 1U, column + 1U);
      const double gy =
          -value(row - 1U, column - 1U) - 2.0 * value(row - 1U, column) -
          value(row - 1U, column + 1U) + value(row + 1U, column - 1U) +
          2.0 * value(row + 1U, column) + value(row + 1U, column + 1U);
      result[row * patch_columns + column] =
          std::min(std::hypot(gx, gy), 255.0);
    }
  }
  return result;
}

[[nodiscard]] Descriptor describe(const SampledPatch &patch,
                                  const bool mouth_annulus) noexcept {
  Descriptor result{};
  result.valid_sample_ratio = static_cast<double>(patch.valid_samples) /
                              static_cast<double>(patch_samples);
  const auto gradient = gradients(patch);
  std::array<double, luma_bins> luma_histogram{};
  std::array<double, chroma_bins> cr_histogram{};
  std::array<double, chroma_bins> cb_histogram{};
  std::array<double, gradient_bins> gradient_histogram{};
  std::array<std::array<double, spatial_channels>, spatial_cells> cell_sums{};
  std::array<std::size_t, spatial_cells> cell_counts{};
  std::size_t included_samples = 0U;

  const auto included = [&](const std::size_t index) noexcept {
    if (!patch.samples[index].valid) {
      return false;
    }
    if (!mouth_annulus) {
      return true;
    }
    return !(std::abs(patch.u[index]) <= 0.68 && patch.v[index] >= -0.25 &&
             patch.v[index] <= 0.28);
  };
  for (std::size_t index = 0U; index < patch_samples; ++index) {
    if (!included(index)) {
      continue;
    }
    const auto histogram_bin = [](const double value,
                                  const std::size_t bins) noexcept {
      return std::min(static_cast<std::size_t>(std::max(value, 0.0) *
                                               static_cast<double>(bins) /
                                               256.0),
                      bins - 1U);
    };
    ++luma_histogram[histogram_bin(patch.samples[index].y, luma_bins)];
    ++cr_histogram[histogram_bin(patch.samples[index].cr, chroma_bins)];
    ++cb_histogram[histogram_bin(patch.samples[index].cb, chroma_bins)];
    ++gradient_histogram[histogram_bin(gradient[index], gradient_bins)];
    ++included_samples;

    std::optional<std::size_t> cell;
    if (mouth_annulus) {
      const bool upper = patch.v[index] >= -0.62 && patch.v[index] < -0.25;
      const bool lower = patch.v[index] >= 0.28 && patch.v[index] <= 0.70;
      if (upper || lower) {
        const std::size_t horizontal = patch.u[index] < -0.32  ? 0U
                                       : patch.u[index] < 0.32 ? 1U
                                                               : 2U;
        cell = (lower ? 3U : 0U) + horizontal;
      }
    } else {
      const std::size_t horizontal = std::min(
          static_cast<std::size_t>(patch.u[index] * 3.0), std::size_t{2U});
      const std::size_t vertical = patch.v[index] < 0.5 ? 0U : 1U;
      cell = vertical * 3U + horizontal;
    }
    if (cell) {
      cell_sums[*cell][0U] += patch.samples[index].y;
      cell_sums[*cell][1U] += patch.samples[index].cr;
      cell_sums[*cell][2U] += patch.samples[index].cb;
      cell_sums[*cell][3U] += gradient[index];
      ++cell_counts[*cell];
    }
  }

  std::size_t output_index = 0U;
  const double histogram_denominator =
      std::max(static_cast<double>(included_samples), 1.0);
  const auto append = [&](const auto &histogram) noexcept {
    for (const double value : histogram) {
      result.values[output_index++] = value / histogram_denominator;
    }
  };
  append(luma_histogram);
  append(cr_histogram);
  append(cb_histogram);
  append(gradient_histogram);
  for (std::size_t cell = 0U; cell < spatial_cells; ++cell) {
    const double denominator =
        std::max(static_cast<double>(cell_counts[cell]), 1.0) * 255.0;
    for (std::size_t channel = 0U; channel < spatial_channels; ++channel) {
      result.values[output_index++] = cell_sums[cell][channel] / denominator;
    }
  }
  return result;
}

[[nodiscard]] double total_variation(const Descriptor &first,
                                     const Descriptor &second,
                                     const std::size_t offset,
                                     const std::size_t count) noexcept {
  double sum = 0.0;
  for (std::size_t index = offset; index < offset + count; ++index) {
    sum += std::abs(first.values[index] - second.values[index]);
  }
  return sum * 0.5;
}

template <std::size_t Size>
[[nodiscard]] double quantile_80(std::array<double, Size> values) noexcept {
  std::sort(values.begin(), values.end());
  const double position = 0.8 * static_cast<double>(values.size() - 1U);
  const auto lower = static_cast<std::size_t>(std::floor(position));
  const auto upper = static_cast<std::size_t>(std::ceil(position));
  const double fraction = position - static_cast<double>(lower);
  return values[lower] * (1.0 - fraction) + values[upper] * fraction;
}

[[nodiscard]] Innovation compare(const Descriptor &current,
                                 const Descriptor &reference) noexcept {
  Innovation result{};
  result.luma_histogram = total_variation(current, reference, 0U, luma_bins);
  result.cr_histogram =
      total_variation(current, reference, luma_bins, chroma_bins);
  result.cb_histogram =
      total_variation(current, reference, luma_bins + chroma_bins, chroma_bins);
  result.gradient_histogram = total_variation(
      current, reference, luma_bins + chroma_bins + chroma_bins, gradient_bins);
  std::array<double, spatial_cells * 3U> color{};
  std::array<double, spatial_cells> texture{};
  std::size_t color_index = 0U;
  for (std::size_t cell = 0U; cell < spatial_cells; ++cell) {
    const auto offset = histogram_values + cell * spatial_channels;
    for (std::size_t channel = 0U; channel < 3U; ++channel) {
      color[color_index++] = std::abs(current.values[offset + channel] -
                                      reference.values[offset + channel]);
    }
    texture[cell] =
        std::abs(current.values[offset + 3U] - reference.values[offset + 3U]);
  }
  result.spatial_color = quantile_80(color);
  result.spatial_texture = quantile_80(texture);
  return result;
}

[[nodiscard]] double
normalized_score(const Innovation &value,
                 const SourceFrameGuardPolicy &policy) noexcept {
  const auto ratio = [](const double numerator,
                        const double denominator) noexcept {
    return numerator / std::max(denominator, epsilon);
  };
  return std::max({
      ratio(value.luma_histogram, policy.maximum_luma_histogram_distance),
      ratio(value.cr_histogram, policy.maximum_chroma_histogram_distance),
      ratio(value.cb_histogram, policy.maximum_chroma_histogram_distance),
      ratio(value.gradient_histogram,
            policy.maximum_gradient_histogram_distance),
      ratio(value.spatial_color, policy.maximum_spatial_color_distance),
      ratio(value.spatial_texture, policy.maximum_spatial_texture_distance),
  });
}

void update_running_mean(Descriptor &baseline, const Descriptor &value,
                         const std::uint32_t previous_count) noexcept {
  const double count = static_cast<double>(previous_count + 1U);
  for (std::size_t index = 0U; index < descriptor_values; ++index) {
    baseline.values[index] +=
        (value.values[index] - baseline.values[index]) / count;
  }
  baseline.valid_sample_ratio +=
      (value.valid_sample_ratio - baseline.valid_sample_ratio) / count;
}

void update_exponential(Descriptor &baseline, const Descriptor &value,
                        const double alpha) noexcept {
  for (std::size_t index = 0U; index < descriptor_values; ++index) {
    baseline.values[index] +=
        (value.values[index] - baseline.values[index]) * alpha;
  }
  baseline.valid_sample_ratio +=
      (value.valid_sample_ratio - baseline.valid_sample_ratio) * alpha;
}

[[nodiscard]] SourceFrameGuardEvidence
make_evidence(const double mouth_baseline, const double mouth_step,
              const double face_baseline, const double face_step,
              const Descriptor &mouth, const Descriptor &face,
              const std::uint32_t samples, const std::uint32_t recovery,
              const std::uint64_t generation) noexcept {
  const double worst = std::max(mouth_baseline, face_baseline);
  return {
      mouth_baseline,
      mouth_step,
      face_baseline,
      face_step,
      mouth.valid_sample_ratio,
      face.valid_sample_ratio,
      std::clamp(1.0 - worst, 0.0, 1.0),
      samples,
      recovery,
      generation,
  };
}

} // namespace

struct SourceFrameAppearanceGuard::State {
  TrackBinding track;
  FrameIdentity last_frame;
  Descriptor mouth_baseline;
  Descriptor face_baseline;
  Descriptor mouth_previous;
  Descriptor face_previous;
  std::uint32_t baseline_observations{};
  std::uint32_t consecutive_recovery_observations{};
  bool rejected{};
  SourceFrameGuardDisposition rejection_disposition{
      SourceFrameGuardDisposition::bypass_mouth_appearance_change};
};

SourceFrameAppearanceGuard::SourceFrameAppearanceGuard(
    const std::uint64_t initial_generation, SourceFrameGuardPolicy policy)
    : policy_(std::move(policy)), active_generation_(initial_generation) {
  const auto positive_or = [](const double value,
                              const double fallback) noexcept {
    return std::isfinite(value) && value > epsilon ? value : fallback;
  };
  policy_.warmup_observations = std::max(policy_.warmup_observations, 2U);
  policy_.recovery_observations = std::max(policy_.recovery_observations, 1U);
  policy_.baseline_alpha = std::isfinite(policy_.baseline_alpha)
                               ? std::clamp(policy_.baseline_alpha, 0.0, 1.0)
                               : 0.12;
  policy_.maximum_luma_histogram_distance =
      positive_or(policy_.maximum_luma_histogram_distance, 0.22);
  policy_.maximum_chroma_histogram_distance =
      positive_or(policy_.maximum_chroma_histogram_distance, 0.18);
  policy_.maximum_gradient_histogram_distance =
      positive_or(policy_.maximum_gradient_histogram_distance, 0.24);
  policy_.maximum_spatial_color_distance =
      positive_or(policy_.maximum_spatial_color_distance, 0.13);
  policy_.maximum_spatial_texture_distance =
      positive_or(policy_.maximum_spatial_texture_distance, 0.12);
  policy_.appearance_trigger_score =
      positive_or(policy_.appearance_trigger_score, 1.0);
  policy_.scene_trigger_score = positive_or(policy_.scene_trigger_score, 1.20);
  policy_.recovery_score = std::isfinite(policy_.recovery_score)
                               ? std::clamp(policy_.recovery_score, 0.0,
                                            policy_.appearance_trigger_score)
                               : 0.75;
  policy_.minimum_valid_sample_ratio =
      std::isfinite(policy_.minimum_valid_sample_ratio)
          ? std::clamp(policy_.minimum_valid_sample_ratio, 0.0, 1.0)
          : 0.98;
}

SourceFrameAppearanceGuard::~SourceFrameAppearanceGuard() = default;
SourceFrameAppearanceGuard::SourceFrameAppearanceGuard(
    SourceFrameAppearanceGuard &&) noexcept = default;
SourceFrameAppearanceGuard &SourceFrameAppearanceGuard::operator=(
    SourceFrameAppearanceGuard &&) noexcept = default;

SourceFrameGuardDecision
SourceFrameAppearanceGuard::evaluate(const CpuFrame &source,
                                     const TrackingEvidence &tracking) {
  if (tracking.track.cancellation_generation != active_generation_) {
    reset_track();
    return {SourceFrameGuardDisposition::bypass_cancelled,
            {.latch_generation = latch_generation_}};
  }
  if (!valid_tracking(tracking)) {
    notify_tracking_loss();
    return {SourceFrameGuardDisposition::bypass_tracking_loss,
            {.latch_generation = latch_generation_}};
  }
  if (!valid_frame(source)) {
    reset_track();
    return {SourceFrameGuardDisposition::bypass_invalid_source,
            {.latch_generation = latch_generation_}};
  }
  if (source.identity != tracking.frame) {
    return {SourceFrameGuardDisposition::bypass_wrong_frame,
            {.latch_generation = latch_generation_}};
  }
  if (state_ && state_->track == tracking.track &&
      (tracking.frame.sequence <= state_->last_frame.sequence ||
       tracking.frame.captured_at_ns <= state_->last_frame.captured_at_ns)) {
    reset_track();
    return {SourceFrameGuardDisposition::bypass_wrong_frame,
            {.latch_generation = latch_generation_}};
  }
  if (state_ && state_->track != tracking.track) {
    reset_track();
  }

  const Descriptor mouth = describe(sample_mouth_patch(source, tracking), true);
  const Descriptor face = describe(sample_face_patch(source, tracking), false);
  if (mouth.valid_sample_ratio < policy_.minimum_valid_sample_ratio ||
      face.valid_sample_ratio < policy_.minimum_valid_sample_ratio) {
    reset_track();
    const auto evidence = make_evidence(0.0, 0.0, 0.0, 0.0, mouth, face, 0U, 0U,
                                        latch_generation_);
    return {SourceFrameGuardDisposition::bypass_invalid_source, evidence};
  }

  if (!state_) {
    state_ = std::make_unique<State>();
    state_->track = tracking.track;
    state_->last_frame = tracking.frame;
    state_->mouth_baseline = mouth;
    state_->face_baseline = face;
    state_->mouth_previous = mouth;
    state_->face_previous = face;
    state_->baseline_observations = 1U;
    return {SourceFrameGuardDisposition::warming_up,
            make_evidence(0.0, 0.0, 0.0, 0.0, mouth, face, 1U, 0U,
                          latch_generation_)};
  }

  state_->last_frame = tracking.frame;
  if (state_->baseline_observations < policy_.warmup_observations) {
    update_running_mean(state_->mouth_baseline, mouth,
                        state_->baseline_observations);
    update_running_mean(state_->face_baseline, face,
                        state_->baseline_observations);
    ++state_->baseline_observations;
    state_->mouth_previous = mouth;
    state_->face_previous = face;
    return {SourceFrameGuardDisposition::warming_up,
            make_evidence(0.0, 0.0, 0.0, 0.0, mouth, face,
                          state_->baseline_observations, 0U,
                          latch_generation_)};
  }

  const double mouth_baseline =
      normalized_score(compare(mouth, state_->mouth_baseline), policy_);
  const double mouth_step =
      normalized_score(compare(mouth, state_->mouth_previous), policy_);
  const double face_baseline =
      normalized_score(compare(face, state_->face_baseline), policy_);
  const double face_step =
      normalized_score(compare(face, state_->face_previous), policy_);
  state_->mouth_previous = mouth;
  state_->face_previous = face;

  const bool scene_trigger = face_baseline > policy_.scene_trigger_score &&
                             face_step > policy_.scene_trigger_score;
  const bool mouth_trigger =
      mouth_baseline > policy_.appearance_trigger_score &&
      mouth_step > policy_.appearance_trigger_score;
  if (!state_->rejected && (scene_trigger || mouth_trigger)) {
    state_->rejected = true;
    state_->consecutive_recovery_observations = 0U;
    state_->rejection_disposition =
        scene_trigger
            ? SourceFrameGuardDisposition::bypass_scene_change
            : SourceFrameGuardDisposition::bypass_mouth_appearance_change;
    const auto evidence = make_evidence(
        mouth_baseline, mouth_step, face_baseline, face_step, mouth, face,
        state_->baseline_observations, 0U, latch_generation_);
    return {state_->rejection_disposition, evidence};
  }

  if (state_->rejected) {
    const bool baseline_consistent =
        state_->rejection_disposition ==
                SourceFrameGuardDisposition::bypass_scene_change
            ? std::max(mouth_baseline, face_baseline) <= policy_.recovery_score
            : mouth_baseline <= policy_.recovery_score;
    state_->consecutive_recovery_observations =
        baseline_consistent ? state_->consecutive_recovery_observations + 1U
                            : 0U;
    if (state_->consecutive_recovery_observations <
        policy_.recovery_observations) {
      const auto disposition =
          baseline_consistent ? SourceFrameGuardDisposition::bypass_recovering
                              : state_->rejection_disposition;
      return {disposition,
              make_evidence(mouth_baseline, mouth_step, face_baseline,
                            face_step, mouth, face,
                            state_->baseline_observations,
                            state_->consecutive_recovery_observations,
                            latch_generation_)};
    }
    state_->rejected = false;
    state_->consecutive_recovery_observations = 0U;
  }

  update_exponential(state_->mouth_baseline, mouth, policy_.baseline_alpha);
  update_exponential(state_->face_baseline, face, policy_.baseline_alpha);
  return {SourceFrameGuardDisposition::accepted,
          make_evidence(mouth_baseline, mouth_step, face_baseline, face_step,
                        mouth, face, state_->baseline_observations, 0U,
                        latch_generation_)};
}

void SourceFrameAppearanceGuard::notify_tracking_loss() noexcept {
  reset_track();
}

bool SourceFrameAppearanceGuard::cancel_to(
    const std::uint64_t generation) noexcept {
  if (generation <= active_generation_) {
    return false;
  }
  active_generation_ = generation;
  reset_track();
  return true;
}

void SourceFrameAppearanceGuard::reset_track() noexcept {
  if (state_) {
    state_.reset();
    ++latch_generation_;
  }
}

std::uint64_t SourceFrameAppearanceGuard::active_generation() const noexcept {
  return active_generation_;
}

std::uint64_t SourceFrameAppearanceGuard::latch_generation() const noexcept {
  return latch_generation_;
}

bool SourceFrameAppearanceGuard::appearance_latched() const noexcept {
  return state_ && !state_->rejected &&
         state_->baseline_observations >= policy_.warmup_observations;
}

} // namespace npc::mouth
