#pragma once

#ifdef YISOU_ENABLE_PERF_TRACE
#include <cstdint>

namespace yisou::perf {

enum class Counter {
    keyboardHook, injectedKeyboardHook, queuedMessage, hotkey, caretTimer,
    selectionTimer, graphicsTimer, svgCacheHit, svgCacheMiss, count
};

class Session {
public:
    Session();
    ~Session();
    Session(const Session&) = delete;
    Session& operator=(const Session&) = delete;
};

struct Memory {
    std::int64_t privateWorkingSet = -1;
    std::int64_t workingSet = -1;
    std::int64_t privateCommit = -1;
};

class Scope {
public:
    explicit Scope(const char* name, bool measureMemory = false);
    ~Scope();
    Scope(const Scope&) = delete;
    Scope& operator=(const Scope&) = delete;

private:
    const char* name_;
    bool measureMemory_;
    std::int64_t started_ = 0;
    Memory before_;
};

void Count(Counter counter);
void Snapshot(const char* name);
bool IsSampleMessage(unsigned message);

}

#define YISOU_PERF_SCOPE(name) ::yisou::perf::Scope performanceScope(name)
#define YISOU_PERF_MEMORY_SCOPE(name) ::yisou::perf::Scope performanceScope(name, true)
#define YISOU_PERF_COUNT(counter) ::yisou::perf::Count(::yisou::perf::Counter::counter)
#define YISOU_PERF_SNAPSHOT(name) ::yisou::perf::Snapshot(name)
#else
#define YISOU_PERF_SCOPE(name) ((void)0)
#define YISOU_PERF_MEMORY_SCOPE(name) ((void)0)
#define YISOU_PERF_COUNT(counter) ((void)0)
#define YISOU_PERF_SNAPSHOT(name) ((void)0)
#endif
