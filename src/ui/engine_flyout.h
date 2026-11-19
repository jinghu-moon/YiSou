#pragma once

#include "application.h"

#include <windows.h>
#include <d2d1.h>
#include <dwrite.h>

namespace yisou::ui {

struct EngineFlyoutActions {
    void (*select)(HWND owner, const EngineId& engine);
    void (*settings)(HWND owner, bool add);
    void (*closed)(HWND flyout);
};

HWND CreateEngineFlyout(HWND anchor, const Application& app, ID2D1Factory& drawingFactory,
    IDWriteFactory& textFactory, IDWriteTextFormat& engineFont, EngineFlyoutActions actions, EngineId selected = {},
    const std::vector<const Engine*>* suggestions = nullptr);
void ShowEngineFlyout(HWND flyout);
bool HandleEngineFlyoutMessage(HWND flyout, const MSG& message);
bool HandleEngineSuggestionKey(HWND flyout, WPARAM key);
bool RefreshEngineSuggestions(HWND flyout, const std::vector<const Engine*>& suggestions);
void RepositionEngineSuggestions(HWND flyout);

}
