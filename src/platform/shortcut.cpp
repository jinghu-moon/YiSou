#include "shortcut.h"
#include "performance.h"

namespace yisou::platform {
namespace {

constexpr DWORD tapInterval = 350;

bool Down(int key)
{
    return (GetAsyncKeyState(key) & 0x8000) != 0;
}

}

bool AltTaps::key(unsigned key, bool down, DWORD time, bool otherModifierHeld)
{
    if (key != VK_LMENU || otherModifierHeld) {
        if (down || key == VK_LMENU) {
            eligible_ = false;
            firstTap_ = false;
        }
        if (key == VK_LMENU && !down) pressed_ = false;
        return false;
    }
    if (down) {
        if (pressed_) return false;
        pressed_ = true;
        eligible_ = true;
        started_ = time;
        if (firstTap_ && time - released_ > tapInterval) firstTap_ = false;
        return false;
    }
    if (!pressed_) return false;
    pressed_ = false;
    if (!eligible_ || time - started_ > tapInterval) {
        firstTap_ = false;
        return false;
    }
    const bool triggered = firstTap_ && time - released_ <= tapInterval;
    firstTap_ = !triggered;
    released_ = time;
    return triggered;
}

bool IsShortcut(Shortcut shortcut)
{
    if (shortcut.key == 0) return shortcut.modifiers == 0;
    if (!(shortcut.modifiers & (shortcutAlt | shortcutControl | shortcutShift | shortcutWin))
        || (shortcut.modifiers & ~(shortcutAlt | shortcutControl | shortcutShift | shortcutWin))) return false;
    return shortcut.key > VK_XBUTTON2 && shortcut.key < 0xff
        && shortcut.key != VK_ESCAPE && shortcut.key != VK_F12 && shortcut.key != VK_PACKET && shortcut.key != VK_PROCESSKEY
        && shortcut.key != VK_SHIFT && shortcut.key != VK_CONTROL && shortcut.key != VK_MENU
        && !(shortcut.key >= VK_LSHIFT && shortcut.key <= VK_RMENU) && shortcut.key != VK_LWIN && shortcut.key != VK_RWIN;
}

unsigned ShortcutModifiers()
{
    return (GetKeyState(VK_MENU) < 0 ? shortcutAlt : 0) | (GetKeyState(VK_CONTROL) < 0 ? shortcutControl : 0)
        | (GetKeyState(VK_SHIFT) < 0 ? shortcutShift : 0)
        | (GetKeyState(VK_LWIN) < 0 || GetKeyState(VK_RWIN) < 0 ? shortcutWin : 0);
}

bool AnyShortcutModifierDown()
{
    return Down(VK_MENU) || Down(VK_CONTROL) || Down(VK_SHIFT) || Down(VK_LWIN) || Down(VK_RWIN);
}

std::wstring ShortcutKeyName(unsigned key)
{
    const auto scan = MapVirtualKeyW(key, MAPVK_VK_TO_VSC_EX);
    const auto packed = static_cast<LONG>((scan & 0xff) << 16) | (scan & 0xff00 ? 1L << 24 : 0);
    wchar_t name[64]{};
    if (GetKeyNameTextW(packed, name, static_cast<int>(std::size(name)))) return name;
    return L"VK " + std::to_wstring(key);
}

ShortcutRegistration* ShortcutRegistration::hooked_ = nullptr;

ShortcutRegistration::ShortcutRegistration(HWND window, UINT triggerMessage, int firstHotkey)
    : window_(window), triggerMessage_(triggerMessage), firstHotkey_(firstHotkey)
{
}

ShortcutRegistration::~ShortcutRegistration()
{
    clear();
}

void ShortcutRegistration::clear()
{
    if (hotkey_) UnregisterHotKey(window_, hotkey_);
    if (hook_) {
        UnhookWindowsHookEx(hook_);
        hooked_ = nullptr;
    }
    hotkey_ = 0;
    hook_ = nullptr;
    taps_.reset();
}

ShortcutResult ShortcutRegistration::apply(Shortcut shortcut, const std::function<bool(Shortcut)>& save)
{
    if (!IsShortcut(shortcut)) return ShortcutResult::unavailable;
    if ((hook_ || hotkey_) && shortcut == shortcut_) {
        return !save || save(shortcut) ? ShortcutResult::applied : ShortcutResult::saveFailed;
    }

    HHOOK newHook = nullptr;
    int newHotkey = 0;
    if (shortcut.key == 0) {
        if (hooked_) return ShortcutResult::unavailable;
        newHook = SetWindowsHookExW(WH_KEYBOARD_LL, KeyboardProc, GetModuleHandleW(nullptr), 0);
        if (!newHook) return ShortcutResult::unavailable;
    } else {
        newHotkey = hotkey_ == firstHotkey_ ? firstHotkey_ + 1 : firstHotkey_;
        if (!RegisterHotKey(window_, newHotkey, shortcut.modifiers | MOD_NOREPEAT, shortcut.key)) return ShortcutResult::unavailable;
    }
    if (save && !save(shortcut)) {
        if (newHook) UnhookWindowsHookEx(newHook);
        if (newHotkey) UnregisterHotKey(window_, newHotkey);
        return ShortcutResult::saveFailed;
    }
    clear();
    hook_ = newHook;
    hotkey_ = newHotkey;
    shortcut_ = shortcut;
    if (hook_) hooked_ = this;
    return ShortcutResult::applied;
}

bool ShortcutRegistration::receives(WPARAM id) const
{
    return hotkey_ && id == static_cast<WPARAM>(hotkey_);
}

void ShortcutRegistration::setRecording(bool value)
{
    recording_ = value;
    taps_.reset();
}

LRESULT CALLBACK ShortcutRegistration::KeyboardProc(int code, WPARAM message, LPARAM data)
{
    if (code == HC_ACTION && hooked_ && !hooked_->recording_) {
        const auto& event = *reinterpret_cast<const KBDLLHOOKSTRUCT*>(data);
        YISOU_PERF_COUNT(keyboardHook);
        if (event.flags & LLKHF_INJECTED) YISOU_PERF_COUNT(injectedKeyboardHook);
        if (!(event.flags & LLKHF_INJECTED)) {
            const bool down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
            const bool modified = event.vkCode == VK_LMENU && (Down(VK_RMENU) || Down(VK_CONTROL)
                || Down(VK_SHIFT) || Down(VK_LWIN) || Down(VK_RWIN));
            if (hooked_->taps_.key(event.vkCode, down, event.time, modified)) {
                PostMessageW(hooked_->window_, hooked_->triggerMessage_, 0, 0);
            }
        }
    }
    return CallNextHookEx(nullptr, code, message, data);
}

}
