#include "performance.h"

#include <windows.h>
#include <psapi.h>
#include <share.h>

#include <array>
#include <cstdio>
#include <cstdlib>
#include <memory>

namespace yisou::perf {
namespace {

// UI and low-level keyboard hook callbacks run on the thread that starts the trace.
std::FILE* logFile = nullptr;
std::int64_t frequency = 0;
std::int64_t origin = 0;
UINT sampleMessage = 0;
std::array<std::uint64_t, static_cast<size_t>(Counter::count)> counters{};

std::int64_t Clock()
{
    LARGE_INTEGER now{};
    QueryPerformanceCounter(&now);
    return now.QuadPart;
}

double Milliseconds(std::int64_t ticks)
{
    return static_cast<double>(ticks) * 1000.0 / static_cast<double>(frequency);
}

Memory ReadMemory()
{
    // PROCESS_MEMORY_COUNTERS_EX2 is newer than the project's Windows SDK.
    struct ExtendedCounters {
        PROCESS_MEMORY_COUNTERS_EX basic;
        SIZE_T privateWorkingSet;
        SIZE_T sharedCommit;
    } memory{};
    const auto process = GetCurrentProcess();
    memory.basic.cb = sizeof(memory);
    const bool extended = K32GetProcessMemoryInfo(process,
        reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&memory), sizeof(memory)) != FALSE;
    if (!extended && !K32GetProcessMemoryInfo(process,
        reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&memory.basic), sizeof(memory.basic))) return {};
    return {extended ? static_cast<std::int64_t>(memory.privateWorkingSet) : -1,
        static_cast<std::int64_t>(memory.basic.WorkingSetSize),
        static_cast<std::int64_t>(memory.basic.PrivateUsage)};
}

std::uint64_t FileTimeTicks(FILETIME value)
{
    return (static_cast<std::uint64_t>(value.dwHighDateTime) << 32) | value.dwLowDateTime;
}

void WriteRow(const char* kind, const char* name, std::int64_t now, double duration,
    Memory before, Memory after, double cpu = -1, std::uint64_t cycles = 0)
{
    std::fprintf(logFile, "%s,%s,%.3f,%.3f,%lld,%lld,%lld,%lld,%lld,%lld,%.3f,%llu", kind, name,
        Milliseconds(now - origin), duration, static_cast<long long>(before.privateWorkingSet),
        static_cast<long long>(after.privateWorkingSet), static_cast<long long>(before.workingSet),
        static_cast<long long>(after.workingSet), static_cast<long long>(before.privateCommit),
        static_cast<long long>(after.privateCommit), cpu, static_cast<unsigned long long>(cycles));
    for (const auto count : counters) std::fprintf(logFile, ",%llu", static_cast<unsigned long long>(count));
    std::fputc('\n', logFile);
}

}

Session::Session()
{
    using Free = decltype(&std::free);
    wchar_t* rawPath = nullptr;
    size_t length = 0;
    if (_wdupenv_s(&rawPath, &length, L"YISOU_PERF_LOG")) return;
    const std::unique_ptr<wchar_t, Free> path(rawPath, std::free);
    if (length <= 1) return;
    LARGE_INTEGER qpcFrequency{};
    if (!QueryPerformanceFrequency(&qpcFrequency)) return;
    logFile = _wfsopen(path.get(), L"wx", _SH_DENYWR);
    if (!logFile) return;
    frequency = qpcFrequency.QuadPart;
    origin = Clock();
    sampleMessage = RegisterWindowMessageW(L"YiSou.Performance.Sample");
    std::fputs("kind,stage,time_ms,duration_ms,private_ws_before,private_ws_after,working_set_before,working_set_after,"
        "private_commit_before,private_commit_after,process_cpu_ms,process_cycles,keyboard_hook,injected_keyboard_hook,queued_message,"
        "hotkey,caret_timer,selection_timer,graphics_timer,svg_cache_hit,svg_cache_miss\n", logFile);
    Snapshot("start");
}

Session::~Session()
{
    if (!logFile) return;
    Snapshot("exit");
    std::fclose(logFile);
    logFile = nullptr;
    sampleMessage = 0;
}

Scope::Scope(const char* name, bool measureMemory) : name_(name), measureMemory_(measureMemory)
{
    if (!logFile) return;
    if (measureMemory_) before_ = ReadMemory();
    started_ = Clock();
}

Scope::~Scope()
{
    if (!started_) return;
    const auto now = Clock();
    WriteRow("scope", name_, now, Milliseconds(now - started_), before_, measureMemory_ ? ReadMemory() : Memory{});
}

void Count(Counter counter)
{
    if (logFile) ++counters[static_cast<size_t>(counter)];
}

void Snapshot(const char* name)
{
    if (!logFile) return;
    const auto now = Clock();
    FILETIME creation{}, exit{}, kernel{}, user{};
    const double cpu = GetProcessTimes(GetCurrentProcess(), &creation, &exit, &kernel, &user)
        ? static_cast<double>(FileTimeTicks(kernel) + FileTimeTicks(user)) / 10000.0 : -1;
    ULONG64 cycles = 0;
    QueryProcessCycleTime(GetCurrentProcess(), &cycles);
    WriteRow("snapshot", name, now, 0, {}, ReadMemory(), cpu, cycles);
    std::fflush(logFile);
}

bool IsSampleMessage(unsigned message)
{
    return sampleMessage && message == sampleMessage;
}

}
