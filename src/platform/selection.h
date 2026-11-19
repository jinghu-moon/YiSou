#pragma once

#include <windows.h>
#include <stop_token>
#include <string>

namespace yisou::platform {

inline constexpr unsigned selectionLimit = 2048;
inline constexpr DWORD selectionCopyTimeoutMs = 850;

struct SelectionCopy {
    HWND clipboardWindow = nullptr;
    unsigned shortcutKey = 0;
    ULONGLONG deadline = 0;
    std::stop_token stop;
};

std::wstring ReadEditSelection(HWND focus);
std::wstring ReadForegroundSelection(HWND foreground, HWND focus, const SelectionCopy& copy = {});
std::wstring ReadClipboardText(HWND window);

}
