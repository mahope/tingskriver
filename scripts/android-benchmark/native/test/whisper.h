#pragma once
#include <cassert>
#include <cstring>

struct whisper_context {};
struct whisper_context_params { bool use_gpu = true; };
struct whisper_full_params {
    int n_threads = 0;
    const char *language = nullptr;
    bool translate = true, no_context = false;
    float temperature = 1, temperature_inc = 1;
    bool print_progress = true, print_realtime = true;
    bool print_timestamps = true, print_special = true;
};
constexpr int WHISPER_SAMPLING_GREEDY = 0;
inline whisper_context_params whisper_context_default_params() { return {}; }
inline whisper_context *whisper_init_from_file_with_params(const char *, whisper_context_params p) {
    assert(!p.use_gpu);
    return new whisper_context;
}
inline void whisper_free(whisper_context *p) { delete p; }
inline whisper_full_params whisper_full_default_params(int strategy) {
    assert(strategy == WHISPER_SAMPLING_GREEDY);
    return {};
}
inline int whisper_full(whisper_context *, whisper_full_params p, const float *pcm, int n) {
    assert(p.n_threads == 4 && std::strcmp(p.language, "da") == 0);
    assert(!p.translate && p.no_context && p.temperature == 0 && p.temperature_inc == 0);
    assert(!p.print_progress && !p.print_realtime && !p.print_timestamps && !p.print_special);
    assert(n == 240000 && pcm[0] == -1.0f && pcm[1] == 0.5f);
    return 0;
}
inline int whisper_full_n_segments(whisper_context *) { return 1; }
inline const char *whisper_full_get_segment_text(whisper_context *, int) { return "..."; }
