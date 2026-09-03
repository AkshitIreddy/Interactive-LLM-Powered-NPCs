#define WIN32_LEAN_AND_MEAN
#define NOMINMAX

#include <windows.h>

#include <moonshine-c-api.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <iomanip>
#include <locale>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>
#include <unordered_set>
#include <utility>
#include <vector>

namespace {

constexpr std::int32_t kBridgeAbiVersion = 1;
constexpr std::size_t kMaximumJsonBytes = 4U * 1024U * 1024U;
thread_local std::string g_last_create_error;

struct BridgeHandle {
  std::mutex mutex;
  std::int32_t transcriber = -1;
  std::int32_t stream = -1;
  bool started = false;
  bool word_timestamps = true;
  std::uint64_t samples_since_poll = 0;
  std::int32_t latest_sample_rate = 16000;
  std::unordered_set<std::uint64_t> completed_lines_emitted;
  std::string last_error;
};

void set_error(BridgeHandle* handle, const std::string& message) {
  if (handle != nullptr) {
    handle->last_error = message.substr(0, 512);
  } else {
    g_last_create_error = message.substr(0, 512);
  }
}

std::string moonshine_error(std::int32_t code) {
  const char* text = moonshine_error_to_string(code);
  return text != nullptr ? std::string(text) : std::string("Moonshine error ") + std::to_string(code);
}

bool check(BridgeHandle* handle, std::int32_t code, const char* stage) {
  if (code == MOONSHINE_ERROR_NONE) {
    return true;
  }
  set_error(handle, std::string(stage) + ": " + moonshine_error(code));
  return false;
}

void append_json_string(std::ostringstream& out, const char* raw) {
  const unsigned char* cursor = reinterpret_cast<const unsigned char*>(raw != nullptr ? raw : "");
  out << '"';
  while (*cursor != 0) {
    const unsigned char value = *cursor++;
    switch (value) {
      case '"': out << "\\\""; break;
      case '\\': out << "\\\\"; break;
      case '\b': out << "\\b"; break;
      case '\f': out << "\\f"; break;
      case '\n': out << "\\n"; break;
      case '\r': out << "\\r"; break;
      case '\t': out << "\\t"; break;
      default:
        if (value < 0x20) {
          out << "\\u" << std::hex << std::setw(4) << std::setfill('0')
              << static_cast<unsigned int>(value) << std::dec << std::setfill(' ');
        } else {
          out << static_cast<char>(value);
        }
    }
  }
  out << '"';
}

std::int64_t seconds_to_ms(float seconds) {
  if (!std::isfinite(seconds) || seconds <= 0.0F) {
    return 0;
  }
  return static_cast<std::int64_t>(std::llround(static_cast<double>(seconds) * 1000.0));
}

std::string transcript_json(BridgeHandle* handle, const transcript_t* transcript) {
  std::ostringstream out;
  out.imbue(std::locale::classic());
  bool speech_started = false;
  bool speech_ended = false;
  bool first_line = true;
  out << "{\"lines\":[";
  if (transcript != nullptr) {
    for (std::uint64_t index = 0; index < transcript->line_count; ++index) {
      const transcript_line_t& line = transcript->lines[index];
      const bool completed_already = handle->completed_lines_emitted.contains(line.id);
      const bool should_emit = line.is_new != 0 || line.is_updated != 0 ||
                               line.has_text_changed != 0 ||
                               (line.is_complete != 0 && !completed_already);
      if (!should_emit) {
        continue;
      }
      speech_started = speech_started || line.is_new != 0;
      speech_ended = speech_ended || (line.is_complete != 0 && !completed_already);
      if (line.is_complete != 0) {
        handle->completed_lines_emitted.insert(line.id);
      }
      if (!first_line) {
        out << ',';
      }
      first_line = false;
      const std::int64_t start_ms = seconds_to_ms(line.start_time);
      const std::int64_t end_ms = start_ms + seconds_to_ms(line.duration);
      out << "{\"utteranceId\":\"moonshine-" << line.id << "\",\"text\":";
      append_json_string(out, line.text);
      out << ",\"startMs\":" << start_ms
          << ",\"endMs\":" << end_ms
          << ",\"complete\":" << (line.is_complete != 0 ? "true" : "false")
          << ",\"changed\":" << (line.has_text_changed != 0 ? "true" : "false")
          << ",\"upstreamLatencyMs\":" << line.last_transcription_latency_ms
          << ",\"words\":[";
      const std::uint64_t emitted_word_count =
          handle->word_timestamps ? line.word_count : 0;
      for (std::uint64_t word_index = 0; word_index < emitted_word_count; ++word_index) {
        if (word_index != 0) {
          out << ',';
        }
        const transcript_word_t& word = line.words[word_index];
        out << "{\"text\":";
        append_json_string(out, word.text);
        out << ",\"startMs\":" << seconds_to_ms(word.start)
            << ",\"endMs\":" << seconds_to_ms(word.end)
            << ",\"confidence\":";
        if (std::isfinite(word.confidence)) {
          out << std::clamp(static_cast<double>(word.confidence), 0.0, 1.0);
        } else {
          out << "null";
        }
        out << '}';
      }
      out << "]}";
    }
  }
  const std::uint64_t analyzed_audio_ms =
      handle->latest_sample_rate > 0
          ? (handle->samples_since_poll * 1000ULL) /
                static_cast<std::uint64_t>(handle->latest_sample_rate)
          : 0;
  handle->samples_since_poll = 0;
  out << "],\"speechStarted\":" << (speech_started ? "true" : "false")
      << ",\"speechEnded\":" << (speech_ended ? "true" : "false")
      << ",\"analyzedAudioMs\":" << analyzed_audio_ms << '}';
  return out.str();
}

std::int32_t allocate_json(BridgeHandle* handle, const std::string& value, char** output) {
  if (output == nullptr) {
    set_error(handle, "output JSON pointer is null");
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  *output = nullptr;
  if (value.size() > kMaximumJsonBytes) {
    set_error(handle, "transcript JSON exceeds the bridge bound");
    return MOONSHINE_ERROR_UNKNOWN;
  }
  auto* allocated = static_cast<char*>(std::malloc(value.size() + 1));
  if (allocated == nullptr) {
    set_error(handle, "transcript JSON allocation failed");
    return MOONSHINE_ERROR_UNKNOWN;
  }
  std::memcpy(allocated, value.data(), value.size());
  allocated[value.size()] = '\0';
  *output = allocated;
  return MOONSHINE_ERROR_NONE;
}

std::int32_t poll_locked(BridgeHandle* handle, char** output) {
  if (!handle->started || handle->stream < 0) {
    set_error(handle, "transcription stream is not started");
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  transcript_t* transcript = nullptr;
  const std::int32_t result = moonshine_transcribe_stream(
      handle->transcriber, handle->stream, 0, &transcript);
  if (!check(handle, result, "transcribe_stream")) {
    return result;
  }
  return allocate_json(handle, transcript_json(handle, transcript), output);
}

}  // namespace

extern "C" {

__declspec(dllexport) std::int32_t npc_stt_bridge_abi_version() {
  return kBridgeAbiVersion;
}

__declspec(dllexport) std::int32_t npc_stt_create(
    const char* model_path, std::int32_t model_arch,
    std::int32_t update_interval_ms, float vad_threshold,
    BridgeHandle** output_handle) {
  if (output_handle == nullptr) {
    set_error(nullptr, "output handle pointer is null");
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  *output_handle = nullptr;
  if (model_path == nullptr || model_path[0] == '\0' ||
      update_interval_ms < 100 || update_interval_ms > 2000 ||
      !std::isfinite(vad_threshold) || vad_threshold < 0.0F || vad_threshold > 1.0F) {
    set_error(nullptr, "bridge creation parameters are invalid");
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }

  auto handle = std::make_unique<BridgeHandle>();
  const std::string interval = std::to_string(update_interval_ms / 1000.0);
  const std::string threshold = std::to_string(vad_threshold);
  std::vector<std::pair<std::string, std::string>> option_storage = {
      {"ort_providers", "CPU"},
      {"identify_speakers", "false"},
      {"word_timestamps", "true"},
      {"decode_incomplete_lines", "true"},
      {"use_speculative_decoding", "true"},
      {"transcription_interval", interval},
      {"vad_threshold", threshold},
      {"save_input_wav_path", ""},
      {"log_ort_run", "false"},
  };
  std::vector<moonshine_option_t> options;
  options.reserve(option_storage.size());
  for (const auto& [name, value] : option_storage) {
    options.push_back({name.c_str(), value.c_str()});
  }
  handle->transcriber = moonshine_load_transcriber_from_files(
      model_path, static_cast<std::uint32_t>(model_arch), options.data(),
      static_cast<std::uint64_t>(options.size()), MOONSHINE_HEADER_VERSION);
  if (handle->transcriber < 0) {
    const std::int32_t result = handle->transcriber;
    set_error(nullptr, std::string("load_transcriber: ") + moonshine_error(result));
    return result;
  }
  handle->stream = moonshine_create_stream(handle->transcriber, 0);
  if (handle->stream < 0) {
    const std::int32_t result = handle->stream;
    set_error(nullptr, std::string("create_stream: ") + moonshine_error(result));
    moonshine_free_transcriber(handle->transcriber);
    return result;
  }
  *output_handle = handle.release();
  return MOONSHINE_ERROR_NONE;
}

__declspec(dllexport) void npc_stt_destroy(BridgeHandle* handle) {
  if (handle == nullptr) {
    return;
  }
  {
    std::lock_guard<std::mutex> guard(handle->mutex);
    if (handle->stream >= 0) {
      moonshine_free_stream(handle->transcriber, handle->stream);
      handle->stream = -1;
    }
    if (handle->transcriber >= 0) {
      moonshine_free_transcriber(handle->transcriber);
      handle->transcriber = -1;
    }
  }
  delete handle;
}

__declspec(dllexport) std::int32_t npc_stt_start(BridgeHandle* handle,
                                                 std::int32_t word_timestamps) {
  if (handle == nullptr) {
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  std::lock_guard<std::mutex> guard(handle->mutex);
  if (handle->started) {
    set_error(handle, "transcription stream is already started");
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  if (handle->stream < 0) {
    handle->stream = moonshine_create_stream(handle->transcriber, 0);
    if (handle->stream < 0) {
      set_error(handle, std::string("create_stream: ") + moonshine_error(handle->stream));
      return handle->stream;
    }
  }
  const std::int32_t result = moonshine_start_stream(handle->transcriber, handle->stream);
  if (!check(handle, result, "start_stream")) {
    return result;
  }
  handle->word_timestamps = word_timestamps != 0;
  handle->started = true;
  handle->samples_since_poll = 0;
  handle->completed_lines_emitted.clear();
  return MOONSHINE_ERROR_NONE;
}

__declspec(dllexport) std::int32_t npc_stt_push_pcm16(
    BridgeHandle* handle, const std::int16_t* samples,
    std::uint64_t sample_count, std::int32_t sample_rate) {
  if (handle == nullptr || samples == nullptr || sample_count == 0 ||
      sample_count > 131072 || sample_rate < 8000 || sample_rate > 48000) {
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  std::vector<float> converted(static_cast<std::size_t>(sample_count));
  std::transform(samples, samples + sample_count, converted.begin(),
                 [](std::int16_t sample) {
                   return static_cast<float>(sample) / 32768.0F;
                 });
  std::lock_guard<std::mutex> guard(handle->mutex);
  if (!handle->started) {
    set_error(handle, "transcription stream is not started");
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  const std::int32_t result = moonshine_transcribe_add_audio_to_stream(
      handle->transcriber, handle->stream, converted.data(), sample_count,
      sample_rate, 0);
  if (!check(handle, result, "add_audio")) {
    return result;
  }
  handle->samples_since_poll += sample_count;
  handle->latest_sample_rate = sample_rate;
  return MOONSHINE_ERROR_NONE;
}

__declspec(dllexport) std::int32_t npc_stt_poll_json(BridgeHandle* handle,
                                                     char** output) {
  if (handle == nullptr) {
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  std::lock_guard<std::mutex> guard(handle->mutex);
  return poll_locked(handle, output);
}

__declspec(dllexport) std::int32_t npc_stt_stop_json(BridgeHandle* handle,
                                                     char** output) {
  if (handle == nullptr) {
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  std::lock_guard<std::mutex> guard(handle->mutex);
  if (!handle->started) {
    set_error(handle, "transcription stream is not started");
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  std::int32_t result = moonshine_stop_stream(handle->transcriber, handle->stream);
  if (!check(handle, result, "stop_stream")) {
    return result;
  }
  transcript_t* transcript = nullptr;
  result = moonshine_transcribe_stream(handle->transcriber, handle->stream, 0, &transcript);
  if (!check(handle, result, "final_transcribe_stream")) {
    return result;
  }
  handle->started = false;
  return allocate_json(handle, transcript_json(handle, transcript), output);
}

__declspec(dllexport) std::int32_t npc_stt_cancel(BridgeHandle* handle) {
  if (handle == nullptr) {
    return MOONSHINE_ERROR_INVALID_ARGUMENT;
  }
  std::lock_guard<std::mutex> guard(handle->mutex);
  if (handle->stream >= 0) {
    moonshine_free_stream(handle->transcriber, handle->stream);
    handle->stream = -1;
  }
  handle->started = false;
  handle->samples_since_poll = 0;
  handle->completed_lines_emitted.clear();
  return MOONSHINE_ERROR_NONE;
}

__declspec(dllexport) void npc_stt_free_json(char* value) {
  std::free(value);
}

__declspec(dllexport) const char* npc_stt_last_error(BridgeHandle* handle) {
  return handle != nullptr ? handle->last_error.c_str() : g_last_create_error.c_str();
}

}  // extern "C"
