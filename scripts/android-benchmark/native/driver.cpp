#include "whisper.h"
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <iterator>
#include <memory>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

using Clock = std::chrono::steady_clock;
static double elapsed(Clock::time_point start) {
    return std::chrono::duration<double, std::milli>(Clock::now() - start).count();
}
static long pss_kib() {
    std::ifstream status("/proc/self/smaps_rollup");
    std::string line;
    while (std::getline(status, line)) {
        if (line.rfind("Pss:", 0) == 0) return std::stol(line.substr(4));
    }
    return -1;
}

int main(int argc, char **argv) {
    if (argc != 3) { std::cerr << "usage: driver MODEL PCM16_16KHZ_MONO\n"; return 2; }
    try {
        std::ifstream input(argv[2], std::ios::binary);
        std::vector<unsigned char> bytes((std::istreambuf_iterator<char>(input)), {});
        if (bytes.size() != 480000) throw std::runtime_error("Expected 15 seconds PCM16");
        std::atomic<bool> stop{false}, unavailable{false};
        std::atomic<long> peak{0};
        std::thread sampler([&] {
            do {
                const long pss = pss_kib();
                if (pss < 0) unavailable = true;
                if (pss > peak) peak = pss;
                std::this_thread::sleep_for(std::chrono::milliseconds(50));
            } while (!stop);
        });
        // Always join the sampler on failure, including model initialization errors.
        struct Join {
            std::atomic<bool> &stop; std::thread &thread;
            ~Join() { stop = true; thread.join(); }
        } join{stop, sampler};
        auto settings = whisper_context_default_params();
        settings.use_gpu = false;
        const auto cold_start = Clock::now();
        std::unique_ptr<whisper_context, decltype(&whisper_free)> context(
            whisper_init_from_file_with_params(argv[1], settings), whisper_free);
        if (!context) throw std::runtime_error("Model load failed");
        const double cold_ms = elapsed(cold_start);
        auto params = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
        params.n_threads = 4;
        params.language = "da";
        params.translate = false;
        params.no_context = true;
        params.temperature = 0;
        params.temperature_inc = 0;
        params.print_progress = false;
        params.print_realtime = false;
        params.print_timestamps = false;
        params.print_special = false;
        // Buffer-ready stop point: conversion and all inference stay inside timing.
        const auto warm_start = Clock::now();
        std::vector<float> pcm(bytes.size() / 2);
        for (size_t i = 0; i < pcm.size(); ++i) {
            int value = bytes[2*i] | (bytes[2*i+1] << 8);
            if (value >= 32768) value -= 65536;
            pcm[i] = value / 32768.0f;
        }
        if (whisper_full(context.get(), params, pcm.data(), static_cast<int>(pcm.size())) != 0)
            throw std::runtime_error("Decode failed");
        std::string text;
        for (int i = 0; i < whisper_full_n_segments(context.get()); ++i)
            text += whisper_full_get_segment_text(context.get(), i);
        const double warm_ms = elapsed(warm_start);
        const long final_pss = pss_kib();
        if (final_pss < 0 || unavailable) throw std::runtime_error("PSS unavailable");
        if (final_pss > peak) peak = final_pss;
        std::cout << cold_ms << '\n' << warm_ms << '\n'
                  << peak.load() * 1024.0 / 1e6 << '\n' << text;
    } catch (const std::exception &) {
        std::cerr << "Native observation failed\n";
        return 2;
    }
}
