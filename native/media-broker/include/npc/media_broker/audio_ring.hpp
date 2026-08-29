#pragma once

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <span>
#include <vector>

namespace npc::media {

enum class PcmSampleKind { unsigned_integer, signed_integer, floating_point };

struct PcmFormat {
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::uint16_t bits_per_sample{};
    std::uint16_t block_align{};
    PcmSampleKind sample_kind{PcmSampleKind::floating_point};

    [[nodiscard]] constexpr bool valid() const noexcept {
        return sample_rate > 0 && channels > 0 && bits_per_sample > 0 && block_align > 0 &&
               block_align >= static_cast<std::uint16_t>(channels * (bits_per_sample / 8));
    }
};

struct PcmTransfer {
    std::uint32_t requested_frames{};
    std::uint32_t transferred_frames{};
    std::uint32_t available_frames{};
    bool overflowed{};
    bool underflowed{};
};

// Single-producer/single-consumer byte ring suitable for shared-memory layout.
// Monotonic 64-bit cursors avoid ambiguous full/empty states. Overflow drops the
// new tail rather than racing the consumer by rewriting unread audio.
class SharedPcmRing final {
public:
    SharedPcmRing(PcmFormat format, std::uint32_t capacity_frames);
    SharedPcmRing(const SharedPcmRing&) = delete;
    SharedPcmRing& operator=(const SharedPcmRing&) = delete;

    [[nodiscard]] PcmTransfer write(std::span<const std::byte> interleaved, std::uint32_t frames) noexcept;
    [[nodiscard]] PcmTransfer read(std::span<std::byte> interleaved, std::uint32_t frames) noexcept;
    void clear() noexcept;

    [[nodiscard]] const PcmFormat& format() const noexcept { return format_; }
    [[nodiscard]] std::uint32_t capacity_frames() const noexcept { return capacity_frames_; }
    [[nodiscard]] std::uint32_t available_frames() const noexcept;
    [[nodiscard]] std::uint64_t overflow_frames() const noexcept { return overflow_frames_.load(); }
    [[nodiscard]] std::uint64_t underflow_frames() const noexcept { return underflow_frames_.load(); }

private:
    void copy_in(std::uint64_t frame_cursor, std::span<const std::byte> bytes, std::uint32_t frames) noexcept;
    void copy_out(std::uint64_t frame_cursor, std::span<std::byte> bytes, std::uint32_t frames) noexcept;

    PcmFormat format_;
    std::uint32_t capacity_frames_{};
    std::vector<std::byte> storage_;
    alignas(64) std::atomic<std::uint64_t> read_cursor_{};
    alignas(64) std::atomic<std::uint64_t> write_cursor_{};
    std::atomic<std::uint64_t> overflow_frames_{};
    std::atomic<std::uint64_t> underflow_frames_{};
};

} // namespace npc::media
