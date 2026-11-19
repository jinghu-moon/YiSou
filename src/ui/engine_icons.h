#pragma once

#include "application.h"

#include <windows.h>
#include <d2d1_1.h>
#include <span>

namespace yisou::ui {

enum class TablerIcon {
    browser, check, chevronDown, chevronUp, eye, fileTypography, keyboard, palette, search, settings, typography, history, plus, count
};

const wchar_t* GroupName(EngineGroup group);
void SetIconDevice(ID2D1Device* device);
IconPixels ReadIconFile(const wchar_t* path);
std::string ReadSvgIconFile(const wchar_t* path);
std::span<const std::uint32_t> EngineIcon(const Engine& engine, unsigned size);
void DrawIconPixels(HDC dc, std::span<const std::uint32_t> pixels, const RECT& bounds, unsigned size);
void DrawEngineIcon(HDC dc, const Engine& engine, const RECT& bounds);
std::span<const std::uint32_t> TablerPixels(TablerIcon icon, COLORREF color, unsigned size);
void DrawTablerIcon(HDC dc, TablerIcon icon, const RECT& bounds, COLORREF color);

}
