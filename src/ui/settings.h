#pragma once

#include "application.h"
#include "../platform/shortcut.h"

#include <windows.h>
#include <d2d1.h>

namespace yisou::ui {

struct SettingsValues {
    FontChoice font;
    LOGFONTW fontFace{};
};

struct SettingsActions {
    bool (*changeFont)(const FontChoice&, LOGFONTW&);
    bool (*changeEngine)(const EngineId&);
    platform::ShortcutResult (*changeShortcut)(Shortcut) = nullptr;
    void (*recordShortcut)(bool) = nullptr;
    bool (*shortcutActive)() = nullptr;
    void (*enginesChanged)() = nullptr;
    platform::ShortcutResult (*changeSelectionShortcut)(const std::optional<SelectionShortcut>&) = nullptr;
    bool (*selectionShortcutActive)() = nullptr;
    void (*scheduleGraphicsCleanup)(HWND owner) = nullptr;
    void (*restartForConfiguration)(HWND owner) = nullptr;
    platform::ShortcutResult (*changeClipboardShortcut)(const std::optional<Shortcut>&) = nullptr;
    bool (*clipboardShortcutActive)() = nullptr;
    void (*restartForLanguage)(HWND owner) = nullptr;
};

HWND CreateSettings(HWND owner, Application& application, ID2D1Factory& factory, const SettingsValues& values, SettingsActions actions);
void ShowSettings(HWND owner, Application& application, ID2D1Factory& factory, const SettingsValues& values, SettingsActions actions);
bool HasOpenSettings();
bool HandleSettingsMessage(MSG& message);
void UpdateSettingsEngine();
void ShowEngineSettingsPage(bool add = false);
void ShowConfigurationSettingsPage();
bool CaptureRegisteredShortcut(Shortcut shortcut);

}
