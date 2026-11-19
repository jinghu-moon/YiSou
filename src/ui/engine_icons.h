#pragma once

#include "application.h"

#include <windows.h>
#include <memory>
#include <span>

namespace yisou::ui {

enum class TablerIcon {
    browser, check, chevronDown, chevronUp, eye, fileTypography, keyboard, palette, search, settings, typography, history, plus, count
};

const wchar_t* GroupName(EngineGroup group);
using SharedIcon = std::shared_ptr<const IconPixels>;
inline constexpr unsigned maxSvgIconSize = 256;
IconPixels RasterizeSvgIcon(std::string_view svg, unsigned size, COLORREF color = CLR_INVALID);
IconPixels ReadIconFile(const wchar_t* path);
std::string ReadSvgIconFile(const wchar_t* path);
SharedIcon EngineIcon(const Engine& engine, unsigned size);
void DrawIconPixels(HDC dc, std::span<const std::uint32_t> pixels, const RECT& bounds, unsigned size);
void DrawEngineIcon(HDC dc, const Engine& engine, const RECT& bounds);
SharedIcon TablerPixels(TablerIcon icon, COLORREF color, unsigned size);
void DrawTablerIcon(HDC dc, TablerIcon icon, const RECT& bounds, COLORREF color);

}
