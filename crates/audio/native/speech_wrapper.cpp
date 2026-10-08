#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstring>
#include <cmath>
#include <exception>
#include <future>
#include <memory>
#include <mutex>
#include <string>

#include "speechapi_cxx.h"

using namespace Microsoft::CognitiveServices::Speech;
using namespace Microsoft::CognitiveServices::Speech::Audio;

namespace {
thread_local std::string g_last_error;

constexpr uint32_t kDefaultSampleRate = 16000;
constexpr uint8_t kDefaultBitsPerSample = 16;
constexpr uint8_t kDefaultChannels = 1;

void set_last_error(const std::string& message) {
  g_last_error = message;
}

int32_t fail(const std::string& message, int32_t code = -1) {
  set_last_error(message);
  return code;
}

void copy_text(const std::string& text, char* out_text, uint32_t out_text_len) {
  if (!out_text || out_text_len == 0) return;
  const size_t max_copy = static_cast<size_t>(out_text_len - 1);
  const size_t to_copy = std::min(max_copy, text.size());
  if (to_copy > 0) {
    std::memcpy(out_text, text.data(), to_copy);
  }
  out_text[to_copy] = '\0';
}

struct BbKeywordRecognizer {
  std::shared_ptr<AudioStreamFormat> format;
  std::shared_ptr<PushAudioInputStream> stream;
  std::shared_ptr<AudioConfig> audio_config;
  std::shared_ptr<KeywordRecognitionModel> model;
  std::shared_ptr<KeywordRecognizer> recognizer;
  std::future<std::shared_ptr<KeywordRecognitionResult>> future;
  std::mutex mutex;
  bool closed = false;
  std::string terminal_error;
};

struct BbAudioFrontend {
  float vad_threshold = 0.01f;
  std::mutex mutex;
  bool closed = false;
};

int32_t ensure_keyword_future(BbKeywordRecognizer* handle) {
  if (!handle) return fail("keyword recognizer handle is null");
  if (handle->closed) return fail("keyword recognizer is closed");
  if (!handle->terminal_error.empty()) return fail(handle->terminal_error, -2);
  if (!handle->future.valid()) {
    handle->future = handle->recognizer->RecognizeOnceAsync(handle->model);
  }
  return 0;
}
}  // namespace

extern "C" const char* bb_last_error() {
  return g_last_error.c_str();
}

extern "C" int32_t bb_keyword_create(const char* model_path,
                                     uint32_t sample_rate,
                                     uint8_t bits_per_sample,
                                     uint8_t channels,
                                     void** out_handle) {
  if (!model_path || !*model_path) return fail("model_path is required");
  if (!out_handle) return fail("out_handle is required");

  try {
    const uint32_t sr = sample_rate == 0 ? kDefaultSampleRate : sample_rate;
    const uint8_t bps = bits_per_sample == 0 ? kDefaultBitsPerSample : bits_per_sample;
    const uint8_t ch = channels == 0 ? kDefaultChannels : channels;

    auto handle = std::make_unique<BbKeywordRecognizer>();
    handle->format = AudioStreamFormat::GetWaveFormatPCM(sr, bps, ch);
    handle->stream = PushAudioInputStream::Create(handle->format);
    handle->audio_config = AudioConfig::FromStreamInput(handle->stream);
    handle->model = KeywordRecognitionModel::FromFile(model_path);
    handle->recognizer = KeywordRecognizer::FromConfig(handle->audio_config);
    handle->future = handle->recognizer->RecognizeOnceAsync(handle->model);

    *out_handle = handle.release();
    g_last_error.clear();
    return 0;
  } catch (const std::exception& ex) {
    return fail(std::string("bb_keyword_create failed: ") + ex.what(), -2);
  } catch (...) {
    return fail("bb_keyword_create failed: unknown error", -2);
  }
}

extern "C" int32_t bb_keyword_write(void* handle_ptr, const uint8_t* data, uint32_t len) {
  if (!handle_ptr) return fail("keyword handle is required");
  if (!data || len == 0) return 0;

  auto* handle = static_cast<BbKeywordRecognizer*>(handle_ptr);
  try {
    std::lock_guard<std::mutex> lock(handle->mutex);
    if (handle->closed) return fail("keyword recognizer is closed");
    if (!handle->terminal_error.empty()) return fail(handle->terminal_error, -2);
    handle->stream->Write(const_cast<uint8_t*>(data), len);
    g_last_error.clear();
    return 0;
  } catch (const std::exception& ex) {
    return fail(std::string("bb_keyword_write failed: ") + ex.what(), -2);
  } catch (...) {
    return fail("bb_keyword_write failed: unknown error", -2);
  }
}

extern "C" int32_t bb_keyword_poll(void* handle_ptr,
                                    uint32_t timeout_ms,
                                    char* out_text,
                                    uint32_t out_text_len,
                                    uint32_t* out_reason) {
  if (!handle_ptr) return fail("keyword handle is required");

  auto* handle = static_cast<BbKeywordRecognizer*>(handle_ptr);
  try {
    std::unique_lock<std::mutex> lock(handle->mutex);
    if (handle->closed) return fail("keyword recognizer is closed");
    const int32_t ensure_result = ensure_keyword_future(handle);
    if (ensure_result != 0) return ensure_result;
    auto& future = handle->future;
    lock.unlock();

    const auto wait_result =
        future.wait_for(std::chrono::milliseconds(timeout_ms));
    if (wait_result != std::future_status::ready) {
      return 1;  // timeout / not ready
    }

    lock.lock();
    if (handle->closed) return fail("keyword recognizer is closed");
    auto result = future.get();
    const auto reason = static_cast<uint32_t>(result->Reason);
    if (out_reason) {
      *out_reason = reason;
    }
    copy_text(result->Text, out_text, out_text_len);
    if (result->Reason == ResultReason::Canceled) {
      auto cancel = CancellationDetails::FromResult(result);
      if (cancel) {
        const auto details = cancel->ErrorDetails;
        if (!details.empty()) {
          set_last_error(details);
        } else {
          set_last_error("Keyword recognition canceled.");
        }
      } else {
        set_last_error("Keyword recognition canceled.");
      }
      // Cancellation is terminal for this handle. A fresh request here (or
      // in a later poll after future.get()) can hang SDK teardown when the
      // original failure was a missing native extension. Preserve the first
      // diagnostic and let the owner dispose of the failed recognizer.
      handle->terminal_error = g_last_error;
      return -2;
    } else {
      g_last_error.clear();
    }

    // Rearm keyword recognition for the next detection.
    handle->future = handle->recognizer->RecognizeOnceAsync(handle->model);
    return 0;
  } catch (const std::exception& ex) {
    return fail(std::string("bb_keyword_poll failed: ") + ex.what(), -2);
  } catch (...) {
    return fail("bb_keyword_poll failed: unknown error", -2);
  }
}

extern "C" int32_t bb_keyword_close(void* handle_ptr) {
  if (!handle_ptr) return 0;
  auto* handle = static_cast<BbKeywordRecognizer*>(handle_ptr);
  try {
    std::lock_guard<std::mutex> lock(handle->mutex);
    handle->closed = true;
    handle->stream->Close();
  } catch (const std::exception& ex) {
    const auto code = fail(std::string("bb_keyword_close failed: ") + ex.what(), -2);
    delete handle;
    return code;
  } catch (...) {
    const auto code = fail("bb_keyword_close failed: unknown error", -2);
    delete handle;
    return code;
  }
  delete handle;
  g_last_error.clear();
  return 0;
}

extern "C" int32_t bb_asr_recognize_once(const char* subscription_key,
                                          const char* region,
                                          const char* language,
                                          const uint8_t* pcm,
                                          uint32_t pcm_len,
                                          uint32_t timeout_ms,
                                          char* out_text,
                                          uint32_t out_text_len,
                                          uint32_t* out_reason) {
  if (!subscription_key || !*subscription_key) return fail("subscription_key is required");
  if (!region || !*region) return fail("region is required");
  if (!pcm || pcm_len == 0) return fail("pcm buffer is required");

  try {
    const std::string lang = (language && *language) ? language : "en-US";

    auto format = AudioStreamFormat::GetWaveFormatPCM(
        kDefaultSampleRate, kDefaultBitsPerSample, kDefaultChannels);
    auto stream = PushAudioInputStream::Create(format);
    auto audio_config = AudioConfig::FromStreamInput(stream);
    auto speech_config = SpeechConfig::FromSubscription(subscription_key, region);
    speech_config->SetSpeechRecognitionLanguage(lang);
    auto recognizer = SpeechRecognizer::FromConfig(speech_config, audio_config);

    auto future = recognizer->RecognizeOnceAsync();
    stream->Write(const_cast<uint8_t*>(pcm), pcm_len);
    stream->Close();

    const auto wait_result =
        future.wait_for(std::chrono::milliseconds(timeout_ms));
    if (wait_result != std::future_status::ready) {
      return 1;  // timeout
    }

    auto result = future.get();
    const auto reason = static_cast<uint32_t>(result->Reason);
    if (out_reason) {
      *out_reason = reason;
    }
    if (result->Reason == ResultReason::RecognizedSpeech) {
      copy_text(result->Text, out_text, out_text_len);
    } else {
      copy_text("", out_text, out_text_len);
    }
    g_last_error.clear();
    return 0;
  } catch (const std::exception& ex) {
    return fail(std::string("bb_asr_recognize_once failed: ") + ex.what(), -2);
  } catch (...) {
    return fail("bb_asr_recognize_once failed: unknown error", -2);
  }
}

extern "C" int32_t bb_frontend_create(float vad_threshold, void** out_handle) {
  if (!out_handle) return fail("out_handle is required");
  try {
    auto handle = std::make_unique<BbAudioFrontend>();
    handle->vad_threshold = vad_threshold > 0.f ? vad_threshold : 0.01f;
    *out_handle = handle.release();
    g_last_error.clear();
    return 0;
  } catch (const std::exception& ex) {
    return fail(std::string("bb_frontend_create failed: ") + ex.what(), -2);
  } catch (...) {
    return fail("bb_frontend_create failed: unknown error", -2);
  }
}

extern "C" int32_t bb_frontend_process(void* handle_ptr,
                                        const uint8_t* data,
                                        uint32_t len,
                                        uint8_t* out_vad_active,
                                        float* out_rms) {
  if (!handle_ptr) return fail("frontend handle is required");
  if (!data || len == 0) {
    if (out_vad_active) *out_vad_active = 0;
    if (out_rms) *out_rms = 0.f;
    return 0;
  }
  if ((len % 2) != 0) return fail("pcm16 input len must be even");

  auto* handle = static_cast<BbAudioFrontend*>(handle_ptr);
  try {
    std::lock_guard<std::mutex> lock(handle->mutex);
    if (handle->closed) return fail("audio frontend is closed");
    const int16_t* samples = reinterpret_cast<const int16_t*>(data);
    const size_t count = static_cast<size_t>(len / 2);
    double sum_sq = 0.0;
    for (size_t i = 0; i < count; ++i) {
      const double sample = static_cast<double>(samples[i]);
      sum_sq += sample * sample;
    }
    const float rms = count > 0
                          ? static_cast<float>(std::sqrt(sum_sq / static_cast<double>(count)) /
                                               32768.0)
                          : 0.f;
    if (out_rms) *out_rms = rms;
    if (out_vad_active) *out_vad_active = rms >= handle->vad_threshold ? 1 : 0;
    g_last_error.clear();
    return 0;
  } catch (const std::exception& ex) {
    return fail(std::string("bb_frontend_process failed: ") + ex.what(), -2);
  } catch (...) {
    return fail("bb_frontend_process failed: unknown error", -2);
  }
}

extern "C" int32_t bb_frontend_close(void* handle_ptr) {
  if (!handle_ptr) return 0;
  auto* handle = static_cast<BbAudioFrontend*>(handle_ptr);
  try {
    std::lock_guard<std::mutex> lock(handle->mutex);
    handle->closed = true;
  } catch (const std::exception& ex) {
    const auto code = fail(std::string("bb_frontend_close failed: ") + ex.what(), -2);
    delete handle;
    return code;
  } catch (...) {
    const auto code = fail("bb_frontend_close failed: unknown error", -2);
    delete handle;
    return code;
  }
  delete handle;
  g_last_error.clear();
  return 0;
}

// Desktop synthesis uses the SDK's word-boundary clock, never estimated timing.
// One Rust worker owns each handle and polls/cancels it; SDK event threads only
// append boundaries under the handle mutex.
#include <vector>
namespace {
struct BbWord {
  std::string text;
  uint64_t start_ms;
  uint64_t duration_ms;
};
struct BbSynthesis {
  // Reverse destruction order keeps callback state alive until SDK destruction.
  std::vector<BbWord> words;
  std::mutex mutex;
  std::shared_ptr<SpeechSynthesisResult> result;
  std::future<std::shared_ptr<SpeechSynthesisResult>> future;
  std::shared_ptr<SpeechSynthesizer> synthesizer;
};
}
extern "C" int32_t bb_tts_start(const char* key, const char* region,
                                const char* ssml, void** out_handle) {
  if (!key || !*key || !region || !*region || !ssml || !*ssml || !out_handle)
    return fail("speech key, region, SSML and output handle are required");
  try {
    auto handle = std::make_unique<BbSynthesis>();
    auto config = SpeechConfig::FromSubscription(key, region);
    config->SetSpeechSynthesisOutputFormat(SpeechSynthesisOutputFormat::Riff48Khz16BitMonoPcm);
    config->SetProperty(PropertyId::SpeechServiceResponse_RequestWordBoundary, "true");
    config->SetProperty(PropertyId::SpeechServiceResponse_RequestSentenceBoundary, "false");
    handle->synthesizer = SpeechSynthesizer::FromConfig(config, nullptr);
    auto* raw = handle.get();
    handle->synthesizer->WordBoundary.Connect([raw](const SpeechSynthesisWordBoundaryEventArgs& e) {
      if (e.BoundaryType != SpeechSynthesisBoundaryType::Word) return;
      std::lock_guard<std::mutex> lock(raw->mutex);
      raw->words.push_back({e.Text, e.AudioOffset / 10000, static_cast<uint64_t>(e.Duration.count())});
    });
    handle->future = handle->synthesizer->SpeakSsmlAsync(ssml);
    *out_handle = handle.release();
    g_last_error.clear();
    return 0;
  } catch (const std::exception& e) { return fail("speech synthesis initialization failed"); }
  catch (...) { return fail("speech synthesis initialization failed"); }
}
extern "C" int32_t bb_tts_poll(void* ptr, uint32_t timeout_ms) {
  if (!ptr) return fail("synthesis handle is required");
  auto* handle = static_cast<BbSynthesis*>(ptr);
  try {
    if (handle->result) return 0;
    if (handle->future.wait_for(std::chrono::milliseconds(timeout_ms)) != std::future_status::ready) return 1;
    handle->result = handle->future.get();
    if (handle->result->Reason != ResultReason::SynthesizingAudioCompleted) {
      auto detail = SpeechSynthesisCancellationDetails::FromResult(handle->result);
      // ErrorDetails may include request headers/provider credentials. Expose only
      // the SDK's typed error code and leave secrets inside this native boundary.
      return fail("speech synthesis canceled (SDK error " + std::to_string(detail ? static_cast<int>(detail->ErrorCode) : -1) + ")");
    }
    g_last_error.clear();
    return 0;
  } catch (const std::exception& e) { return fail("speech synthesis failed"); }
  catch (...) { return fail("speech synthesis failed"); }
}
extern "C" int32_t bb_tts_audio(void* ptr, const uint8_t** out_bytes, uint32_t* out_length) {
  if (!ptr || !out_bytes || !out_length) return fail("synthesis audio arguments missing");
  auto* handle = static_cast<BbSynthesis*>(ptr);
  if (!handle->result || handle->result->Reason != ResultReason::SynthesizingAudioCompleted) return fail("synthesis is incomplete");
  auto bytes = handle->result->GetAudioData();
  *out_bytes = bytes->data();
  *out_length = static_cast<uint32_t>(bytes->size());
  return 0;
}
extern "C" int32_t bb_tts_word(void* ptr, uint32_t index, char* text,
                               uint32_t text_capacity, uint64_t* start_ms, uint64_t* duration_ms) {
  if (!ptr || !text || !start_ms || !duration_ms) return fail("synthesis word arguments missing");
  auto* handle = static_cast<BbSynthesis*>(ptr);
  std::lock_guard<std::mutex> lock(handle->mutex);
  if (index >= handle->words.size()) return 1;
  const auto& word = handle->words[index];
  if (word.text.size() + 1 > text_capacity) return fail("word exceeds output capacity");
  copy_text(word.text, text, text_capacity);
  *start_ms = word.start_ms; *duration_ms = word.duration_ms;
  return 0;
}
extern "C" int32_t bb_tts_close(void* ptr) {
  if (!ptr) return 0;
  std::unique_ptr<BbSynthesis> handle(static_cast<BbSynthesis*>(ptr));
  try {
    handle->synthesizer->StopSpeakingAsync().get();
    handle->synthesizer->WordBoundary.DisconnectAll();
    // Destroy synthesizer before the event callback's target fields.
    handle->synthesizer.reset();
    return 0;
  } catch (...) {
    handle->synthesizer->WordBoundary.DisconnectAll();
    handle->synthesizer.reset();
    return fail("speech synthesis cleanup failed");
  }
}
