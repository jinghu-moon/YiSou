#include "selection.h"
#include "shortcut.h"

#include <UIAutomationClient.h>
#include <ole2.h>
#include <oleauto.h>
#include <wrl/client.h>

#include <algorithm>
#include <vector>

namespace yisou::platform {
namespace {
using Microsoft::WRL::ComPtr;

constexpr DWORD clipboardOpenRetryMs = 50;
constexpr DWORD firstCopyWaitMs = 150;
constexpr DWORD lastCopyWaitMs = 220;
constexpr DWORD copyBudgetMs = clipboardOpenRetryMs + firstCopyWaitMs + lastCopyWaitMs;

enum class SelectionWindow { other, chromium, windowsTerminal, console };

SelectionWindow ClassifyWindow(HWND window)
{
    wchar_t name[40]{};
    GetClassNameW(window, name, static_cast<int>(std::size(name)));
    if (wcscmp(name, L"CASCADIA_HOSTING_WINDOW_CLASS") == 0) return SelectionWindow::windowsTerminal;
    if (wcscmp(name, L"ConsoleWindowClass") == 0) return SelectionWindow::console;
    if (wcscmp(name, L"Chrome_WidgetWin_1") == 0) return SelectionWindow::chromium;
    return SelectionWindow::other;
}

std::wstring ReadRanges(IUIAutomationTextPattern& pattern, bool terminal = false)
{
    ComPtr<IUIAutomationTextRangeArray> ranges;
    if (FAILED(pattern.GetSelection(ranges.GetAddressOf())) || !ranges) return {};
    int count = 0;
    if (FAILED(ranges->get_Length(&count))) return {};
    std::wstring selection;
    for (int index = 0; index < count && selection.size() + 1 < selectionLimit; ++index) {
        ComPtr<IUIAutomationTextRange> range;
        if (FAILED(ranges->GetElement(index, range.GetAddressOf())) || !range) continue;
        // Terminal can return readable selected text even when CompareEndpoints fails.
        if (!terminal) {
            int difference = 0;
            if (SUCCEEDED(range->CompareEndpoints(TextPatternRangeEndpoint_Start, range.Get(),
                TextPatternRangeEndpoint_End, &difference)) && difference == 0) continue;
        }
        BSTR text = nullptr;
        const int room = static_cast<int>(selectionLimit - selection.size()) - (selection.empty() ? 0 : 1);
        if (SUCCEEDED(range->GetText(room, &text)) && text && SysStringLen(text)) {
            if (!selection.empty()) selection.push_back(L' ');
            selection.append(text, std::min<size_t>(SysStringLen(text), selectionLimit - selection.size()));
        }
        SysFreeString(text);
    }
    return selection;
}

std::wstring ReadElementSelection(IUIAutomationElement& element, bool terminal = false)
{
    ComPtr<IUIAutomationTextPattern> pattern;
    if (FAILED(element.GetCurrentPatternAs(UIA_TextPatternId, IID_PPV_ARGS(pattern.GetAddressOf()))) || !pattern) return {};
    return ReadRanges(*pattern.Get(), terminal);
}

std::wstring ReadTerminalSelection(IUIAutomation& automation, HWND foreground, SelectionWindow kind)
{
    ComPtr<IUIAutomationElement> window;
    if (FAILED(automation.ElementFromHandle(foreground, window.GetAddressOf())) || !window) return {};
    if (kind == SelectionWindow::console) return ReadElementSelection(*window.Get(), true);

    // Terminal splits can retain independent selections. Prefer the focused terminal pane.
    VARIANT controlName{}, offscreen{};
    controlName.vt = VT_BSTR;
    controlName.bstrVal = SysAllocString(L"TermControl");
    if (!controlName.bstrVal) return {};
    ComPtr<IUIAutomationCondition> isTerminal, isVisible, condition;
    const HRESULT nameResult = automation.CreatePropertyCondition(UIA_ClassNamePropertyId, controlName, isTerminal.GetAddressOf());
    VariantClear(&controlName);
    offscreen.vt = VT_BOOL;
    offscreen.boolVal = VARIANT_FALSE;
    if (FAILED(nameResult)
        || FAILED(automation.CreatePropertyCondition(UIA_IsOffscreenPropertyId, offscreen, isVisible.GetAddressOf()))
        || FAILED(automation.CreateAndCondition(isTerminal.Get(), isVisible.Get(), condition.GetAddressOf()))) return {};

    ComPtr<IUIAutomationElementArray> panes;
    if (FAILED(window->FindAll(TreeScope_Descendants, condition.Get(), panes.GetAddressOf())) || !panes) return {};
    int count = 0;
    if (FAILED(panes->get_Length(&count))) return {};
    for (int index = 0; index < count; ++index) {
        ComPtr<IUIAutomationElement> pane;
        if (FAILED(panes->GetElement(index, pane.GetAddressOf())) || !pane) continue;
        BOOL focused = FALSE;
        if (count == 1 || (SUCCEEDED(pane->get_CurrentHasKeyboardFocus(&focused)) && focused)) {
            return ReadElementSelection(*pane.Get(), true);
        }
    }
    return {};
}

std::wstring ReadDocumentSelection(IUIAutomation& automation, HWND foreground, int attempts, ULONGLONG deadline)
{
    ComPtr<IUIAutomationElement> window, document;
    if (FAILED(automation.ElementFromHandle(foreground, window.GetAddressOf())) || !window) return {};
    VARIANT type{}, offscreen{}, textAvailable{};
    type.vt = VT_I4;
    type.lVal = UIA_DocumentControlTypeId;
    offscreen.vt = textAvailable.vt = VT_BOOL;
    offscreen.boolVal = VARIANT_FALSE;
    textAvailable.boolVal = VARIANT_TRUE;
    ComPtr<IUIAutomationCondition> isDocument, isVisible, hasText, visibleDocument, condition;
    if (FAILED(automation.CreatePropertyCondition(UIA_ControlTypePropertyId, type, isDocument.GetAddressOf()))
        || FAILED(automation.CreatePropertyCondition(UIA_IsOffscreenPropertyId, offscreen, isVisible.GetAddressOf()))
        || FAILED(automation.CreatePropertyCondition(UIA_IsTextPatternAvailablePropertyId, textAvailable, hasText.GetAddressOf()))
        || FAILED(automation.CreateAndCondition(isDocument.Get(), isVisible.Get(), visibleDocument.GetAddressOf()))
        || FAILED(automation.CreateAndCondition(visibleDocument.Get(), hasText.Get(), condition.GetAddressOf()))) return {};

    // Chromium may populate its accessibility tree asynchronously on the first query.
    for (int attempt = 0; attempt < attempts && GetTickCount64() < deadline && GetForegroundWindow() == foreground; ++attempt) {
        if (FAILED(window->FindFirst(TreeScope_Descendants, condition.Get(), document.ReleaseAndGetAddressOf()))) return {};
        if (document) break;
        if (attempt + 1 < attempts) Sleep(35);
    }
    if (!document) return {};

    return ReadElementSelection(*document.Get());
}

std::wstring ReadAutomationSelection(HWND foreground, SelectionWindow kind, const SelectionCopy& copy, bool& passwordField)
{
    const bool copyFallback = copy.clipboardWindow != nullptr;
    const auto deadline = copyFallback ? copy.deadline - std::min<ULONGLONG>(copy.deadline, copyBudgetMs) : MAXULONGLONG;
    const auto canRead = [&] {
        return GetTickCount64() < deadline && !copy.stop.stop_requested() && GetForegroundWindow() == foreground;
    };
    if (!canRead()) return {};
    ComPtr<IUIAutomation> automation;
    if (FAILED(CoCreateInstance(CLSID_CUIAutomation8, nullptr, CLSCTX_INPROC_SERVER,
        IID_PPV_ARGS(automation.GetAddressOf())))) return {};
    ComPtr<IUIAutomationElement> element;
    const bool terminal = kind == SelectionWindow::windowsTerminal || kind == SelectionWindow::console;
    if (copyFallback) {
        ComPtr<IUIAutomation2> timeouts;
        if (SUCCEEDED(automation.As(&timeouts))) {
            // Leave time for the copy fallback if an accessibility provider stalls.
            timeouts->put_ConnectionTimeout(150);
            timeouts->put_TransactionTimeout(150);
        }
    }
    if ((FAILED(automation->GetFocusedElement(element.GetAddressOf())) || !element) && !terminal) return {};
    ComPtr<IUIAutomationTreeWalker> parents;
    automation->get_RawViewWalker(parents.GetAddressOf());
    for (int depth = 0; element && depth < 8 && canRead(); ++depth) {
        BOOL password = FALSE;
        if (FAILED(element->get_CurrentIsPassword(&password))) {
            if (!terminal) return {};
            break;
        }
        if (password) {
            passwordField = true;
            return {};
        }
        CONTROLTYPEID type = 0;
        element->get_CurrentControlType(&type);
        ComPtr<IUIAutomationTextPattern> pattern;
        if (SUCCEEDED(element->GetCurrentPatternAs(UIA_TextPatternId, IID_PPV_ARGS(pattern.GetAddressOf()))) && pattern) {
            auto text = ReadRanges(*pattern.Get(), terminal);
            if (!text.empty()) return text;
            if (terminal) break;
        }
        if (type == UIA_EditControlTypeId || type == UIA_DocumentControlTypeId) {
            if (!terminal) return {};
            break;
        }
        ComPtr<IUIAutomationElement> parent;
        if (type == UIA_WindowControlTypeId || !parents
            || FAILED(parents->GetParentElement(element.Get(), parent.GetAddressOf()))) break;
        element = std::move(parent);
    }
    if (!canRead()) return {};
    if (terminal) return ReadTerminalSelection(*automation.Get(), foreground, kind);
    // Alt can put focus on the browser toolbar while the page retains its selection.
    // Other apps can use Ctrl+C without a potentially slow document-tree search.
    const bool chromiumWindow = kind == SelectionWindow::chromium;
    if (copyFallback && !chromiumWindow) return {};
    return ReadDocumentSelection(*automation.Get(), foreground, chromiumWindow ? 4 : 1, deadline);
}

class OpenedClipboard {
public:
    explicit OpenedClipboard(HWND window, DWORD waitMs = 0) : open_(OpenClipboard(window) != FALSE)
    {
        const auto deadline = GetTickCount64() + waitMs;
        while (!open_ && GetTickCount64() < deadline) {
            Sleep(5);
            open_ = OpenClipboard(window) != FALSE;
        }
    }
    ~OpenedClipboard() { if (open_) CloseClipboard(); }
    OpenedClipboard(const OpenedClipboard&) = delete;
    OpenedClipboard& operator=(const OpenedClipboard&) = delete;
    explicit operator bool() const { return open_; }

private:
    bool open_;
};

class ClipboardBackup {
public:
    ClipboardBackup() = default;
    ClipboardBackup(const ClipboardBackup&) = delete;
    ClipboardBackup& operator=(const ClipboardBackup&) = delete;

    ~ClipboardBackup()
    {
        for (const auto& [format, data] : formats_) {
            if (!data) continue;
            switch (format) {
            case CF_BITMAP:
            case CF_PALETTE: DeleteObject(data); break;
            case CF_ENHMETAFILE: DeleteEnhMetaFile(static_cast<HENHMETAFILE>(data)); break;
            case CF_METAFILEPICT:
                if (const auto picture = static_cast<const METAFILEPICT*>(GlobalLock(data))) {
                    DeleteMetaFile(picture->hMF);
                    GlobalUnlock(data);
                }
                [[fallthrough]];
            default: GlobalFree(data); break;
            }
        }
    }

    bool save()
    {
        UINT format = 0;
        for (;;) {
            SetLastError(ERROR_SUCCESS);
            format = EnumClipboardFormats(format);
            if (!format) return GetLastError() == ERROR_SUCCESS;
            // These formats may hold app-owned handles that cannot be duplicated safely.
            if (format == CF_OWNERDISPLAY || (format >= CF_DSPTEXT && format <= CF_DSPENHMETAFILE)
                || (format >= CF_PRIVATEFIRST && format <= CF_GDIOBJLAST)) return false;
            formats_.emplace_back(format, nullptr);
            const auto data = GetClipboardData(format);
            if (!data) return false;
            formats_.back().second = OleDuplicateData(data, static_cast<CLIPFORMAT>(format), GMEM_MOVEABLE);
            if (!formats_.back().second) return false;
        }
    }

    void restore()
    {
        if (!EmptyClipboard()) return;
        for (auto& [format, data] : formats_) {
            if (SetClipboardData(format, data)) data = nullptr;
        }
    }

private:
    std::vector<std::pair<UINT, HANDLE>> formats_;
};

size_t CopyClipboardText(wchar_t (&buffer)[selectionLimit])
{
    const auto data = GetClipboardData(CF_UNICODETEXT);
    if (!data) return 0;
    const auto* text = static_cast<const wchar_t*>(GlobalLock(data));
    if (!text) return 0;
    const auto bound = std::min<size_t>(std::size(buffer), GlobalSize(data) / sizeof(wchar_t));
    const auto end = std::find(text, text + bound, L'\0');
    const auto length = static_cast<size_t>(end - text);
    std::copy(text, end, buffer);
    GlobalUnlock(data);
    return length;
}

DWORD WindowProcess(HWND window)
{
    DWORD process = 0;
    if (window) GetWindowThreadProcessId(window, &process);
    return process;
}

bool SendCopyShortcut(WORD key)
{
    INPUT keys[4]{};
    for (auto& input : keys) input.type = INPUT_KEYBOARD;
    keys[0].ki.wVk = keys[3].ki.wVk = VK_LCONTROL;
    keys[1].ki.wVk = keys[2].ki.wVk = key;
    keys[2].ki.dwFlags = keys[3].ki.dwFlags = KEYEVENTF_KEYUP;
    if (key == VK_INSERT) {
        keys[1].ki.dwFlags |= KEYEVENTF_EXTENDEDKEY;
        keys[2].ki.dwFlags |= KEYEVENTF_EXTENDEDKEY;
    }
    const UINT sent = SendInput(static_cast<UINT>(std::size(keys)), keys, sizeof(INPUT));
    if (sent && sent < std::size(keys)) {
        // Release any keys left pressed by a partial SendInput.
        SendInput(sent == 2 ? 2 : 1, sent == 2 ? &keys[2] : &keys[3], sizeof(INPUT));
    }
    return sent != 0;
}

std::wstring ReadSelectionByCopy(HWND foreground, HWND focus, const SelectionCopy& copy, SelectionWindow kind)
{
    const bool windowsTerminal = kind == SelectionWindow::windowsTerminal;
    const auto canCopy = [&] {
        if (copy.stop.stop_requested() || GetTickCount64() >= copy.deadline
            || GetForegroundWindow() != foreground) return false;
        GUITHREADINFO gui{sizeof(gui)};
        return GetGUIThreadInfo(GetWindowThreadProcessId(foreground, nullptr), &gui) && (!focus || gui.hwndFocus == focus);
    };
    const auto keysDown = [&] {
        return AnyShortcutModifierDown() || (GetAsyncKeyState(static_cast<int>(copy.shortcutKey)) & 0x8000)
            || (GetAsyncKeyState(VK_INSERT) & 0x8000) || (GetAsyncKeyState('C') & 0x8000);
    };
    while (canCopy() && keysDown()) Sleep(5);
    if (!canCopy()) return {};

    ClipboardBackup backup;
    DWORD sequence = 0;
    HWND owner = nullptr;
    {
        const OpenedClipboard clipboard(copy.clipboardWindow, clipboardOpenRetryMs);
        if (!clipboard || !canCopy() || keysDown() || !backup.save()) return {};
        sequence = GetClipboardSequenceNumber();
        owner = GetClipboardOwner();
    }
    const DWORD sourceProcess = WindowProcess(foreground);
    if (!sourceProcess) return {};

    const auto clipboardChanged = [&] {
        return GetClipboardSequenceNumber() != sequence || GetClipboardOwner() != owner;
    };
    // Only send Ctrl+Insert in Terminal. Ctrl+C could interrupt a command there.
    constexpr WORD copyKeys[] = {'C', VK_INSERT};
    for (size_t attempt = windowsTerminal ? 1 : 0; attempt < std::size(copyKeys); ++attempt) {
        if (!canCopy() || keysDown() || clipboardChanged() || !SendCopyShortcut(copyKeys[attempt])) return {};

        // Finish a copy already in flight even if the search UI cancels the request.
        const auto copyDeadline = GetTickCount64() + (attempt == 0 ? firstCopyWaitMs : lastCopyWaitMs);
        do {
            if (clipboardChanged()) {
                const OpenedClipboard clipboard(copy.clipboardWindow, clipboardOpenRetryMs);
                if (!clipboard) return {};
                if (!clipboardChanged()) continue;
                const HWND copiedOwner = GetClipboardOwner();
                if (copiedOwner) {
                    if (WindowProcess(copiedOwner) != sourceProcess) return {};
                } else if (!windowsTerminal || GetClipboardSequenceNumber() == sequence || !canCopy()) {
                    // Windows Terminal can publish text without an owner HWND.
                    return {};
                }
                wchar_t buffer[selectionLimit];
                const auto length = CopyClipboardText(buffer);
                if (!copiedOwner && (!length || !canCopy())) return {};
                backup.restore();
                return canCopy() ? std::wstring(buffer, length) : std::wstring{};
            }
            Sleep(5);
        } while (GetTickCount64() < copyDeadline);
    }
    return {};
}

}

std::wstring ReadEditSelection(HWND focus)
{
    wchar_t kind[16]{};
    if (!focus || !GetClassNameW(focus, kind, static_cast<int>(std::size(kind))) || _wcsicmp(kind, L"Edit") != 0
        || (GetWindowLongPtrW(focus, GWL_STYLE) & ES_PASSWORD)) return {};
    constexpr UINT flags = SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT;
    DWORD start = 0, end = 0;
    if (!SendMessageTimeoutW(focus, EM_GETSEL, reinterpret_cast<WPARAM>(&start),
        reinterpret_cast<LPARAM>(&end), flags, 80, nullptr) || start >= end || start > 1024 * 1024) return {};
    const auto length = std::min<DWORD>(end - start, selectionLimit);
    std::wstring text(start + length + 1, L'\0');
    DWORD_PTR copied = 0;
    if (!SendMessageTimeoutW(focus, WM_GETTEXT, text.size(), reinterpret_cast<LPARAM>(text.data()), flags, 80, &copied)
        || copied <= start) return {};
    return text.substr(start, std::min<size_t>(length, copied - start));
}

std::wstring ReadForegroundSelection(HWND foreground, HWND focus, const SelectionCopy& copy)
{
    if (!foreground || GetForegroundWindow() != foreground) return {};
    wchar_t focusClass[16]{};
    if (focus && GetClassNameW(focus, focusClass, static_cast<int>(std::size(focusClass))) && _wcsicmp(focusClass, L"Edit") == 0
        && (GetWindowLongPtrW(focus, GWL_STYLE) & ES_PASSWORD)) return {};
    if (auto text = ReadEditSelection(focus); !text.empty()) {
        return GetForegroundWindow() == foreground ? text : std::wstring{};
    }
    const auto kind = ClassifyWindow(foreground);
    bool passwordField = false;
    if (SUCCEEDED(CoInitializeEx(nullptr, COINIT_MULTITHREADED))) {
        auto text = ReadAutomationSelection(foreground, kind, copy, passwordField);
        CoUninitialize();
        if (!text.empty()) return GetForegroundWindow() == foreground ? text : std::wstring{};
    }
    // Ctrl+C in a console can interrupt its running command.
    if (passwordField || !copy.clipboardWindow || kind == SelectionWindow::console) return {};
    return ReadSelectionByCopy(foreground, focus, copy, kind);
}

std::wstring ReadClipboardText(HWND window)
{
    const OpenedClipboard clipboard(window);
    if (!clipboard) return {};
    wchar_t buffer[selectionLimit];
    return std::wstring(buffer, CopyClipboardText(buffer));
}

}
