#include "npc/media_broker/audio_ring.hpp"

#include <algorithm>
#include <cstring>
#include <stdexcept>

namespace npc::media {

SharedPcmRing::SharedPcmRing(PcmFormat format, const std::uint32_t capacity_frames)
    : format_(format),
      capacity_frames_(capacity_frames),
      storage_(static_cast<std::size_t>(capacity_frames) * format.block_align) {
    if (!format.valid() || capacity_frames == 0) {
        throw std::invalid_argument("SharedPcmRing requires a valid format and non-zero capacity");
    }
}

std::uint32_t SharedPcmRing::available_frames() const noexcept {
    const auto write = write_cursor_.load(std::memory_order_acquire);
    const auto read = read_cursor_.load(std::memory_order_acquire);
    return static_cast<std::uint32_t>(std::min<std::uint64_t>(write - read, capacity_frames_));
}

PcmTransfer SharedPcmRing::write(const std::span<const std::byte> interleaved,
                                 const std::uint32_t frames) noexcept {
    const auto required_bytes = static_cast<std::size_t>(frames) * format_.block_align;
    if (interleaved.size() < required_bytes) {
        return {frames, 0, available_frames(), true, false};
    }

    const auto write = write_cursor_.load(std::memory_order_relaxed);
    const auto read = read_cursor_.load(std::memory_order_acquire);
    const auto occupied = std::min<std::uint64_t>(write - read, capacity_frames_);
    const auto free = capacity_frames_ - static_cast<std::uint32_t>(occupied);
    const auto accepted = std::min(frames, free);
    if (accepted > 0) {
        copy_in(write, interleaved, accepted);
        write_cursor_.store(write + accepted, std::memory_order_release);
    }
    if (accepted < frames) {
        overflow_frames_.fetch_add(frames - accepted, std::memory_order_relaxed);
    }
    return {frames, accepted, static_cast<std::uint32_t>(occupied) + accepted, accepted < frames, false};
}

PcmTransfer SharedPcmRing::read(const std::span<std::byte> interleaved,
                                const std::uint32_t frames) noexcept {
    const auto required_bytes = static_cast<std::size_t>(frames) * format_.block_align;
    if (interleaved.size() < required_bytes) {
        return {frames, 0, available_frames(), false, true};
    }

    auto read = read_cursor_.load(std::memory_order_relaxed);
    const auto write = write_cursor_.load(std::memory_order_acquire);
    const auto available = static_cast<std::uint32_t>(
        std::min<std::uint64_t>(write - read, capacity_frames_));
    const auto delivered = std::min(frames, available);
    if (delivered > 0) {
        copy_out(read, interleaved, delivered);
        // clear() can advance the consumer cursor from the control thread during
        // cancellation. If it wins this race, discard and zero the copied stale
        // bytes instead of resurrecting pre-cancellation speech.
        if (!read_cursor_.compare_exchange_strong(read, read + delivered,
                                                  std::memory_order_release,
                                                  std::memory_order_acquire)) {
            std::fill_n(interleaved.data(),
                        static_cast<std::size_t>(delivered) * format_.block_align,
                        std::byte{});
            underflow_frames_.fetch_add(frames, std::memory_order_relaxed);
            return {frames, 0, available_frames(), false, true};
        }
    }
    if (delivered < frames) {
        underflow_frames_.fetch_add(frames - delivered, std::memory_order_relaxed);
    }
    return {frames, delivered, available - delivered, false, delivered < frames};
}

void SharedPcmRing::clear() noexcept {
    const auto write = write_cursor_.load(std::memory_order_acquire);
    read_cursor_.store(write, std::memory_order_release);
}

void SharedPcmRing::copy_in(const std::uint64_t frame_cursor,
                            const std::span<const std::byte> bytes,
                            const std::uint32_t frames) noexcept {
    const auto first_frame = static_cast<std::uint32_t>(frame_cursor % capacity_frames_);
    const auto first_count = std::min(frames, capacity_frames_ - first_frame);
    const auto first_bytes = static_cast<std::size_t>(first_count) * format_.block_align;
    std::memcpy(storage_.data() + static_cast<std::size_t>(first_frame) * format_.block_align,
                bytes.data(), first_bytes);
    const auto remaining = frames - first_count;
    if (remaining > 0) {
        std::memcpy(storage_.data(), bytes.data() + first_bytes,
                    static_cast<std::size_t>(remaining) * format_.block_align);
    }
}

void SharedPcmRing::copy_out(const std::uint64_t frame_cursor,
                             const std::span<std::byte> bytes,
                             const std::uint32_t frames) noexcept {
    const auto first_frame = static_cast<std::uint32_t>(frame_cursor % capacity_frames_);
    const auto first_count = std::min(frames, capacity_frames_ - first_frame);
    const auto first_bytes = static_cast<std::size_t>(first_count) * format_.block_align;
    std::memcpy(bytes.data(),
                storage_.data() + static_cast<std::size_t>(first_frame) * format_.block_align,
                first_bytes);
    const auto remaining = frames - first_count;
    if (remaining > 0) {
        std::memcpy(bytes.data() + first_bytes, storage_.data(),
                    static_cast<std::size_t>(remaining) * format_.block_align);
    }
}

} // namespace npc::media
