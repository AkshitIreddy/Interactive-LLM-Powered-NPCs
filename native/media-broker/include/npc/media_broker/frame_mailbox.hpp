#pragma once

#include <cstdint>
#include <mutex>
#include <optional>
#include <utility>

namespace npc::media {

// A bounded latest-value mailbox. Producers never block behind GPU consumers;
// replacing an unread item is intentional and counted as a dropped stale item.
template <typename T>
class LatestValueMailbox {
public:
    struct PushResult {
        std::uint64_t generation{};
        bool replaced_unread{};
    };

    [[nodiscard]] PushResult push(T value) {
        std::scoped_lock lock(mutex_);
        const bool replaced = value_.has_value();
        value_ = std::move(value);
        ++generation_;
        if (replaced) {
            ++dropped_;
        }
        return {generation_, replaced};
    }

    [[nodiscard]] std::optional<T> take_latest() {
        std::scoped_lock lock(mutex_);
        auto result = std::move(value_);
        value_.reset();
        return result;
    }

    void clear() {
        std::scoped_lock lock(mutex_);
        value_.reset();
    }

    [[nodiscard]] std::uint64_t generation() const {
        std::scoped_lock lock(mutex_);
        return generation_;
    }

    [[nodiscard]] std::uint64_t dropped() const {
        std::scoped_lock lock(mutex_);
        return dropped_;
    }

private:
    mutable std::mutex mutex_;
    std::optional<T> value_;
    std::uint64_t generation_{};
    std::uint64_t dropped_{};
};

} // namespace npc::media
