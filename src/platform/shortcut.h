#pragma once

#include "application.h"

#include <functional>
#include <windows.h>

namespace yisou::platform {

class AltTaps {
public:
    bool key(unsigned key, bool down, DWORD time, bool otherModifierHeld);
    void reset() { *this = {}; }

private:
    bool pressed_ = false;
    bool eligible_ = false;
    bool firstTap_ = false;
    DWORD started_ = 0;
    DWORD released_ = 0;
};

bool IsShortcut(Shortcut shortcut);
unsigned ShortcutModifiers();
bool AnyShortcutModifierDown();
std::wstring ShortcutKeyName(unsigned key);

enum class ShortcutResult { applied, unavailable, saveFailed };

class ShortcutRegistration {
public:
    ShortcutRegistration(HWND window, UINT triggerMessage, int firstHotkey = 1);
    ~ShortcutRegistration();
    ShortcutRegistration(const ShortcutRegistration&) = delete;
    ShortcutRegistration& operator=(const ShortcutRegistration&) = delete;

    ShortcutResult apply(Shortcut shortcut, const std::function<bool(Shortcut)>& save = {});
    void clear();
    bool receives(WPARAM id) const;
    void setRecording(bool value);

private:
    static LRESULT CALLBACK KeyboardProc(int code, WPARAM message, LPARAM data);
    static ShortcutRegistration* hooked_;

    HWND window_;
    UINT triggerMessage_;
    int firstHotkey_;
    HHOOK hook_ = nullptr;
    int hotkey_ = 0;
    Shortcut shortcut_{};
    bool recording_ = false;
    AltTaps taps_;
};

}
