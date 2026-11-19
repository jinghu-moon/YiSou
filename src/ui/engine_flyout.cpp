#include "../platform/localization.h"
#include "engine_flyout.h"
#include "engine_icons.h"
#include "visual_style.h"

#include <commctrl.h>
#include <dwmapi.h>
#include <windowsx.h>
#include <wrl/client.h>

#include <algorithm>
#include <new>

namespace yisou::ui {
using i18n::Text;
using i18n::Tr;

namespace {
using Microsoft::WRL::ComPtr;

constexpr wchar_t className[] = L"YiSouEngineFlyout";
constexpr int panelWidth = 352;
constexpr int inset = 8;
constexpr int gridColumns = 6;
constexpr int tileWidth = 86;
constexpr int tileHeight = 90;
constexpr int iconTileEdge = 64;
constexpr int tileGap = 6;
constexpr int gridInset = 10;
constexpr int maxGridRows = 3;
constexpr int rowHeight = 34;
constexpr int headerHeight = 29;
constexpr UINT dismiss = WM_APP + 1;
constexpr int suggestionsId = 1;
constexpr int firstTileId = 100;

struct Palette {
    static D2D1_COLOR_F Color(COLORREF value)
    {
        return D2D1::ColorF(GetRValue(value) / 255.f, GetGValue(value) / 255.f, GetBValue(value) / 255.f);
    }
    D2D1_COLOR_F surface = Color(panelSurface);
    D2D1_COLOR_F grid = Color(panelRail);
    D2D1_COLOR_F tile = D2D1::ColorF(0xfafbfd);
    D2D1_COLOR_F border = Color(panelBorder);
    D2D1_COLOR_F text = D2D1::ColorF(0x202124);
    D2D1_COLOR_F secondary = D2D1::ColorF(0x5e6570);
    D2D1_COLOR_F hover = Color(panelHover);
    D2D1_COLOR_F selected = Color(panelSelection);
    D2D1_COLOR_F selectedText = D2D1::ColorF(0x202124);
    D2D1_COLOR_F accent = Color(panelAccent);
};

D2D1_COLOR_F SystemColor(int index)
{
    const COLORREF color = GetSysColor(index);
    return D2D1::ColorF(GetRValue(color) / 255.f, GetGValue(color) / 255.f, GetBValue(color) / 255.f);
}

Palette GetPalette()
{
    HIGHCONTRASTW contrast{sizeof(contrast)};
    if (!SystemParametersInfoW(SPI_GETHIGHCONTRAST, sizeof(contrast), &contrast, 0)
        || !(contrast.dwFlags & HCF_HIGHCONTRASTON)) return {};
    const auto surface = SystemColor(COLOR_WINDOW);
    const auto text = SystemColor(COLOR_WINDOWTEXT);
    const auto highlight = SystemColor(COLOR_HIGHLIGHT);
    return {surface, surface, highlight, text, text, text, SystemColor(COLOR_BTNFACE), highlight,
        SystemColor(COLOR_HIGHLIGHTTEXT), SystemColor(COLOR_HIGHLIGHTTEXT)};
}

struct CreateInfo {
    const Application& app;
    ID2D1Factory& drawingFactory;
    IDWriteFactory& textFactory;
    IDWriteTextFormat& engineFont;
    EngineFlyoutActions actions;
    EngineId selected;
    const std::vector<const Engine*>* suggestions;
};

struct Flyout {
    const Application& app;
    EngineFlyoutActions actions;
    EngineId selected;
    ComPtr<ID2D1Factory> factory;
    ComPtr<IDWriteTextFormat> engineFont;
    ComPtr<IDWriteTextFormat> detailFont;
    ComPtr<IDWriteTextFormat> tileFont;
    ComPtr<IDWriteTextFormat> aliasFont;
    ComPtr<ID2D1DCRenderTarget> target;
    ComPtr<ID2D1SolidColorBrush> brush;
    Palette colors = GetPalette();
    bool suggesting;
    bool showNames;
    std::vector<size_t> visibleEngines;
    HWND window = nullptr;
    HWND engineList = nullptr;
    HWND scrollbar = nullptr;
    HWND tooltips = nullptr;
    std::vector<HWND> tiles;
    int columns = gridColumns;
    int pageRows = 1;
    int topRow = 0;
    int selectedTile = 0;
    int wheelRemainder = 0;
    UINT dpi = USER_DEFAULT_SCREEN_DPI;
    HWND hoveredControl = nullptr;
    int hoveredRow = -1;
    int pressedRow = -1;

    Flyout(const CreateInfo& info, HWND handle)
        : app(info.app), actions(info.actions), selected(info.selected), factory(&info.drawingFactory), engineFont(&info.engineFont),
          suggesting(info.suggestions != nullptr), showNames(info.app.preferences().showEngineTileNames), window(handle)
    {
        dpi = GetDpiForWindow(window);
        if (!dpi) dpi = USER_DEFAULT_SCREEN_DPI;
        if (suggesting) {
            for (const auto* engine : *info.suggestions) visibleEngines.push_back(engine - app.engines().data());
        } else {
            const auto& engines = app.engines();
            for (const auto& id : app.preferences().pinnedEngines) {
                if (const auto* engine = app.findEngine(id)) visibleEngines.push_back(engine - engines.data());
            }
            for (size_t index = 0; index < engines.size(); ++index) {
                if (!app.isPinned(engines[index].id)) visibleEngines.push_back(index);
            }
            const auto active = std::ranges::find_if(visibleEngines, [&](size_t index) {
                return engines[index].id == selected;
            });
            selectedTile = static_cast<int>(active - visibleEngines.begin());
            if (showNames) CreateTileFonts(info.textFactory);
            return;
        }
        info.textFactory.CreateTextFormat(L"Segoe UI Variable Text", nullptr, DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 12.f, i18n::LocaleName(), detailFont.GetAddressOf());
        if (detailFont) detailFont->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    }

    int Px(int value) const { return MulDiv(value, dpi, USER_DEFAULT_SCREEN_DPI); }
    float Dip(int value) const { return static_cast<float>(value) * USER_DEFAULT_SCREEN_DPI / dpi; }
    int TileCount() const { return static_cast<int>(visibleEngines.size()) + 1; }
    int Rows() const { return (TileCount() + columns - 1) / columns; }
    int TileWidth() const { return Px(showNames ? tileWidth : iconTileEdge); }
    int TileHeight() const { return Px(showNames ? tileHeight : iconTileEdge); }

    void CreateTileFonts(IDWriteFactory& textFactory)
    {
        const auto make = [&](ComPtr<IDWriteTextFormat>& format, float size, DWRITE_FONT_WEIGHT weight,
            DWRITE_TEXT_ALIGNMENT alignment) {
            if (FAILED(textFactory.CreateTextFormat(L"Segoe UI Variable Text", nullptr, weight,
                DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, i18n::LocaleName(), format.GetAddressOf()))) return;
            format->SetTextAlignment(alignment);
            format->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            format->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            const DWRITE_TRIMMING trimming{DWRITE_TRIMMING_GRANULARITY_CHARACTER, 0, 0};
            ComPtr<IDWriteInlineObject> ellipsis;
            if (SUCCEEDED(textFactory.CreateEllipsisTrimmingSign(format.Get(), ellipsis.GetAddressOf()))) {
                format->SetTrimming(&trimming, ellipsis.Get());
            }
        };
        make(tileFont, 13.f, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER);
        make(aliasFont, 11.f, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING);
    }

    bool Begin(HDC dc, const RECT& bounds)
    {
        if (IsRectEmpty(&bounds)) return false;
        if (!target) {
            const auto options = D2D1::RenderTargetProperties(D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_IGNORE));
            if (FAILED(factory->CreateDCRenderTarget(&options, target.GetAddressOf()))) return false;
            if (FAILED(target->CreateSolidColorBrush(colors.text, brush.GetAddressOf()))) {
                target.Reset();
                return false;
            }
        }
        if (FAILED(target->BindDC(dc, &bounds))) return false;
        target->SetDpi(static_cast<float>(dpi), static_cast<float>(dpi));
        target->BeginDraw();
        return true;
    }

    void End()
    {
        if (FAILED(target->EndDraw())) {
            brush.Reset();
            target.Reset();
        }
    }

    void Fill(D2D1_RECT_F rect, float radius, D2D1_COLOR_F color)
    {
        brush->SetColor(color);
        target->FillRoundedRectangle(D2D1::RoundedRect(rect, radius, radius), brush.Get());
    }

    void Text(std::wstring_view text, IDWriteTextFormat* font, D2D1_RECT_F bounds, D2D1_COLOR_F color)
    {
        if (!font) return;
        brush->SetColor(color);
        target->DrawText(text.data(), static_cast<UINT32>(text.size()), font, bounds, brush.Get(), D2D1_DRAW_TEXT_OPTIONS_CLIP);
    }
};

Flyout* GetFlyout(HWND window)
{
    return reinterpret_cast<Flyout*>(GetWindowLongPtrW(window, GWLP_USERDATA));
}

void PopulateEngines(Flyout& flyout)
{
    SendMessageW(flyout.engineList, WM_SETREDRAW, FALSE, 0);
    ListBox_ResetContent(flyout.engineList);
    for (const auto index : flyout.visibleEngines) {
        const auto& engine = flyout.app.engines()[index];
        const int row = ListBox_AddString(flyout.engineList, i18n::EngineName(engine.id, engine.name.c_str()));
        if (row < 0) break;
    }
    ListBox_SetCurSel(flyout.engineList, 0);
    SendMessageW(flyout.engineList, WM_SETREDRAW, TRUE, 0);
    InvalidateRect(flyout.engineList, nullptr, TRUE);
}

COLORREF Rgb(D2D1_COLOR_F color)
{
    return RGB(color.r * 255.f, color.g * 255.f, color.b * 255.f);
}

void PaintPanel(Flyout& flyout, HDC dc)
{
    RECT bounds{};
    GetClientRect(flyout.window, &bounds);
    if (!flyout.Begin(dc, bounds)) return;
    const auto& palette = flyout.colors;
    const auto width = flyout.Dip(bounds.right);
    const auto height = flyout.Dip(bounds.bottom);
    flyout.target->Clear(flyout.suggesting ? palette.surface : palette.grid);
    flyout.brush->SetColor(palette.border);
    const float radius = flyout.suggesting ? cardRadius : 12.f;
    flyout.target->DrawRoundedRectangle(D2D1::RoundedRect(
        D2D1::RectF(.5f, .5f, width - .5f, height - .5f), radius, radius), flyout.brush.Get());
    if (flyout.suggesting) {
        flyout.Text(Tr(Text::searchEngines), flyout.detailFont.Get(), D2D1::RectF(16, 4, width - 94, headerHeight + 4), palette.secondary);
        flyout.Text(Tr(Text::tabToConfirm), flyout.detailFont.Get(), D2D1::RectF(width - 75, 4, width - 12, headerHeight + 4), palette.secondary);
    }
    flyout.End();
}

void PaintTile(Flyout& flyout, const DRAWITEMSTRUCT& item)
{
    const int index = static_cast<int>(item.CtlID) - firstTileId;
    if (index < 0 || index >= flyout.TileCount()) return;
    const auto& bounds = item.rcItem;
    if (!flyout.Begin(item.hDC, bounds)) return;

    const auto& palette = flyout.colors;
    const bool add = index == flyout.TileCount() - 1;
    const auto* engine = add ? nullptr : &flyout.app.engines()[flyout.visibleEngines[index]];
    const bool current = engine && index == flyout.selectedTile;
    const bool pressed = (item.itemState & ODS_SELECTED) != 0;
    const bool focused = (item.itemState & (ODS_FOCUS | ODS_NOFOCUSRECT)) == ODS_FOCUS;
    const bool hovered = flyout.hoveredControl == item.hwndItem;
    const auto w = flyout.Dip(bounds.right - bounds.left);
    const auto h = flyout.Dip(bounds.bottom - bounds.top);
    const auto card = D2D1::RectF(.5f, .5f, w - .5f, h - .5f);
    flyout.target->Clear(palette.grid);
    if (pressed) flyout.Fill(card, 8.f, palette.selected);
    else if (current) flyout.Fill(card, 8.f, palette.tile);
    else if (hovered || focused) flyout.Fill(card, 8.f, palette.hover);
    if (focused || current) {
        flyout.brush->SetColor(focused ? palette.accent : palette.border);
        flyout.target->DrawRoundedRectangle(D2D1::RoundedRect(card, 8.f, 8.f), flyout.brush.Get());
    }
    if (flyout.showNames) {
        const auto ink = pressed || current ? palette.selectedText : palette.text;
        if (engine && !engine->alias.empty()) {
            flyout.Text(L"#" + engine->alias, flyout.aliasFont.Get(),
                D2D1::RectF(7, 4, w - 8, 20), current ? palette.accent : palette.secondary);
        }
        flyout.Text(engine ? i18n::EngineName(engine->id, engine->name.c_str()) : Tr(Text::add),
            flyout.tileFont.Get(), D2D1::RectF(5, 55, w - 5, 75), ink);
    }
    if (current) flyout.Fill(D2D1::RectF(w / 2 - 8, h - 10, w / 2 + 8, h - 7), 1.5f, palette.accent);
    flyout.End();

    const LONG size = flyout.Px(flyout.showNames ? 28 : 36);
    const LONG x = bounds.left + (bounds.right - bounds.left - size) / 2;
    const LONG y = flyout.showNames ? bounds.top + flyout.Px(23) : (bounds.top + bounds.bottom - size) / 2;
    const RECT icon{x, y, x + size, y + size};
    if (engine) DrawEngineIcon(item.hDC, *engine, icon);
    else DrawTablerIcon(item.hDC, TablerIcon::plus, icon, Rgb(palette.secondary));
}

void PaintSuggestion(Flyout& flyout, const DRAWITEMSTRUCT& item)
{
    if (item.itemID >= flyout.visibleEngines.size()) return;
    const auto& engine = flyout.app.engines()[flyout.visibleEngines[item.itemID]];
    const bool current = (item.itemState & ODS_SELECTED) != 0;
    const bool hovered = flyout.hoveredControl == item.hwndItem && static_cast<UINT>(flyout.hoveredRow) == item.itemID;
    const auto& bounds = item.rcItem;
    const auto& palette = flyout.colors;
    if (!flyout.Begin(item.hDC, bounds)) return;
    const float w = flyout.Dip(bounds.right - bounds.left);
    const float h = flyout.Dip(bounds.bottom - bounds.top);
    flyout.target->Clear(palette.surface);
    const auto row = D2D1::RectF(1, 2, w - 1, h - 2);
    if (current || hovered) flyout.Fill(row, controlRadius, current ? palette.selected : palette.hover);
    const auto alias = engine.alias.empty() ? std::wstring{} : L"#" + engine.alias;
    const float aliasStart = std::max(86.f, w - 29.f - std::min(98.f, 10.f + 6.f * static_cast<float>(alias.size())));
    flyout.Text(i18n::EngineName(engine.id, engine.name.c_str()), flyout.engineFont.Get(),
        D2D1::RectF(40, 0, alias.empty() ? w - 29 : aliasStart - 4, h), current ? palette.selectedText : palette.text);
    if (!alias.empty()) {
        flyout.Text(alias, flyout.detailFont.Get(), D2D1::RectF(aliasStart, 0, w - 29, h), palette.secondary);
    }
    flyout.End();

    const LONG left = bounds.left + flyout.Px(10);
    const LONG size = flyout.Px(24);
    const LONG top = (bounds.top + bounds.bottom - size) / 2;
    DrawEngineIcon(item.hDC, engine, {left, top, left + size, top + size});
    if (current) DrawTablerIcon(item.hDC, TablerIcon::check,
        {bounds.right - flyout.Px(29), bounds.top, bounds.right - flyout.Px(11), bounds.bottom}, Rgb(palette.accent));
}

int RowAt(HWND list, POINT point)
{
    const LRESULT hit = SendMessageW(list, LB_ITEMFROMPOINT, 0, MAKELPARAM(point.x, point.y));
    RECT row{};
    return !HIWORD(hit) && ListBox_GetItemRect(list, LOWORD(hit), &row) != LB_ERR && PtInRect(&row, point)
        ? LOWORD(hit) : -1;
}

void InvokeEngine(Flyout& flyout, int index)
{
    if (index < 0 || index >= static_cast<int>(flyout.visibleEngines.size())) return;
    const auto engine = flyout.app.engines()[flyout.visibleEngines[index]].id;
    const auto select = flyout.actions.select;
    const HWND owner = GetWindow(flyout.window, GW_OWNER);
    DestroyWindow(flyout.window);
    select(owner, engine);
}

void OpenEngineSettings(Flyout& flyout, bool add)
{
    const auto settings = flyout.actions.settings;
    const HWND owner = GetWindow(flyout.window, GW_OWNER);
    DestroyWindow(flyout.window);
    if (settings) settings(owner, add);
}

void InvokeTile(Flyout& flyout, int index)
{
    if (index == flyout.TileCount() - 1) OpenEngineSettings(flyout, true);
    else InvokeEngine(flyout, index);
}

LRESULT CALLBACK ControlProc(HWND control, UINT message, WPARAM key, LPARAM data, UINT_PTR, DWORD_PTR context)
{
    auto& flyout = *reinterpret_cast<Flyout*>(context);
    const POINT point{GET_X_LPARAM(data), GET_Y_LPARAM(data)};
    switch (message) {
    case WM_MOUSEMOVE: {
        const int row = flyout.suggesting ? RowAt(control, point) : -1;
        if (flyout.hoveredControl != control) {
            TRACKMOUSEEVENT tracking{sizeof(tracking), TME_LEAVE, control, 0};
            TrackMouseEvent(&tracking);
            if (flyout.hoveredControl) InvalidateRect(flyout.hoveredControl, nullptr, FALSE);
        }
        if (flyout.hoveredControl != control || flyout.hoveredRow != row) {
            flyout.hoveredControl = control;
            flyout.hoveredRow = row;
            InvalidateRect(control, nullptr, FALSE);
        }
        break;
    }
    case WM_MOUSELEAVE:
        if (flyout.hoveredControl == control) flyout.hoveredControl = nullptr;
        InvalidateRect(control, nullptr, FALSE);
        break;
    case WM_LBUTTONDOWN:
        if (flyout.suggesting) {
            flyout.pressedRow = RowAt(control, point);
            return 0;
        }
        break;
    case WM_LBUTTONUP:
        if (flyout.suggesting) {
            const int row = RowAt(control, point);
            const bool chosen = row >= 0 && row == flyout.pressedRow;
            flyout.pressedRow = -1;
            if (chosen) InvokeEngine(flyout, row);
            return 0;
        }
        break;
    case WM_CANCELMODE:
        flyout.pressedRow = -1;
        break;
    }
    return DefSubclassProc(control, message, key, data);
}

bool CreateControls(Flyout& flyout)
{
    const auto instance = GetModuleHandleW(nullptr);
    const auto create = [&](const wchar_t* kind, const wchar_t* name, DWORD style, int id) -> HWND {
        const HWND control = CreateWindowExW(0, kind, name, WS_CHILD | style, 0, 0, 0, 0, flyout.window,
            reinterpret_cast<HMENU>(static_cast<UINT_PTR>(id)), instance, nullptr);
        if (!control || !SetWindowSubclass(control, ControlProc, 1, reinterpret_cast<DWORD_PTR>(&flyout))) return nullptr;
        return control;
    };
    if (flyout.suggesting) {
        const DWORD listStyle = WS_VISIBLE | WS_VSCROLL | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT | LBS_HASSTRINGS | LBS_OWNERDRAWFIXED;
        flyout.engineList = create(L"LISTBOX", Tr(Text::searchEngines), listStyle, suggestionsId);
        if (!flyout.engineList) return false;
        PopulateEngines(flyout);
        return true;
    }
    if (!flyout.showNames) {
        flyout.tooltips = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, TOOLTIPS_CLASSW, nullptr,
            WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX, 0, 0, 0, 0, flyout.window, nullptr, instance, nullptr);
    }
    for (int i = 0; i < flyout.TileCount(); ++i) {
        const auto* name = Tr(Text::addSearchEngine);
        if (i < static_cast<int>(flyout.visibleEngines.size())) {
            const auto& engine = flyout.app.engines()[flyout.visibleEngines[i]];
            name = i18n::EngineName(engine.id, engine.name.c_str());
        }
        const auto tile = create(L"BUTTON", name, WS_TABSTOP | BS_OWNERDRAW, firstTileId + i);
        if (!tile) return false;
        flyout.tiles.push_back(tile);
        if (flyout.tooltips) {
            TOOLINFOW tool{sizeof(tool)};
            tool.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
            tool.hwnd = flyout.window;
            tool.uId = reinterpret_cast<UINT_PTR>(tile);
            tool.lpszText = LPSTR_TEXTCALLBACKW;
            SendMessageW(flyout.tooltips, TTM_ADDTOOLW, 0, reinterpret_cast<LPARAM>(&tool));
        }
    }
    return true;
}

void Layout(Flyout& flyout)
{
    RECT bounds{};
    GetClientRect(flyout.window, &bounds);
    if (flyout.suggesting) {
        if (!flyout.engineList) return;
        const int padding = flyout.Px(inset);
        const int top = flyout.Px(headerHeight + 4);
        MoveWindow(flyout.engineList, padding, top, std::max(0L, bounds.right - 2 * padding),
            std::max(0L, bounds.bottom - top - padding), TRUE);
        return;
    }

    const int padding = flyout.Px(gridInset);
    const int gap = flyout.Px(tileGap);
    const int height = flyout.TileHeight();
    const int scrollbarWidth = flyout.scrollbar ? GetSystemMetricsForDpi(SM_CXVSCROLL, flyout.dpi) : 0;
    const int available = std::max(0L, bounds.right - 2 * padding - (flyout.columns - 1) * gap - scrollbarWidth);
    if (flyout.scrollbar) {
        MoveWindow(flyout.scrollbar, bounds.right - padding - scrollbarWidth, padding, scrollbarWidth,
            std::max(0L, bounds.bottom - 2 * padding), TRUE);
    }
    for (int i = 0; i < static_cast<int>(flyout.tiles.size()); ++i) {
        const int row = i / flyout.columns - flyout.topRow;
        const auto tile = flyout.tiles[i];
        if (row < 0 || row >= flyout.pageRows) {
            ShowWindow(tile, SW_HIDE);
            continue;
        }
        const int column = i % flyout.columns;
        const int left = padding + column * gap + column * available / flyout.columns;
        const int right = padding + column * gap + (column + 1) * available / flyout.columns;
        SetWindowPos(tile, nullptr, left, padding + row * (height + gap), right - left, height,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW);
    }
    InvalidateRect(flyout.window, nullptr, FALSE);
}

void ScrollTo(Flyout& flyout, int row)
{
    const int next = std::clamp(row, 0, std::max(0, flyout.Rows() - flyout.pageRows));
    if (next == flyout.topRow) return;
    flyout.topRow = next;
    flyout.hoveredControl = nullptr;
    if (flyout.tooltips) SendMessageW(flyout.tooltips, TTM_POP, 0, 0);
    if (flyout.scrollbar) SetScrollPos(flyout.scrollbar, SB_CTL, next, TRUE);
    const auto focused = std::ranges::find(flyout.tiles, GetFocus());
    if (focused != flyout.tiles.end()) {
        const int focusedRow = static_cast<int>(focused - flyout.tiles.begin()) / flyout.columns;
        if (focusedRow < next || focusedRow >= next + flyout.pageRows) SetFocus(flyout.window);
    }
    Layout(flyout);
}

void FocusTile(Flyout& flyout, int index)
{
    const int row = index / flyout.columns;
    if (row < flyout.topRow) ScrollTo(flyout, row);
    else if (row >= flyout.topRow + flyout.pageRows) ScrollTo(flyout, row - flyout.pageRows + 1);
    SetFocus(flyout.tiles[index]);
}

LRESULT CALLBACK FlyoutProc(HWND window, UINT message, WPARAM key, LPARAM data)
{
    if (message == WM_NCCREATE) {
        auto& info = *static_cast<CreateInfo*>(reinterpret_cast<CREATESTRUCTW*>(data)->lpCreateParams);
        auto* flyout = new (std::nothrow) Flyout(info, window);
        if (!flyout) return FALSE;
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(flyout));
    }
    auto* flyout = GetFlyout(window);
    if (!flyout) return DefWindowProcW(window, message, key, data);
    switch (message) {
    case WM_NCCALCSIZE:
        return 0;
    case WM_NCHITTEST:
        return HTCLIENT;
    case WM_MOUSEACTIVATE:
        if (flyout->suggesting) return MA_NOACTIVATE;
        break;
    case WM_CREATE:
        return CreateControls(*flyout) ? 0 : -1;
    case WM_SIZE:
        Layout(*flyout);
        return 0;
    case WM_SYSCOLORCHANGE:
    case WM_SETTINGCHANGE:
        flyout->colors = GetPalette();
        RedrawWindow(window, nullptr, nullptr, RDW_INVALIDATE | RDW_ALLCHILDREN);
        break;
    case WM_MEASUREITEM:
        reinterpret_cast<MEASUREITEMSTRUCT*>(data)->itemHeight = flyout->Px(rowHeight);
        return TRUE;
    case WM_DRAWITEM: {
        const auto& item = *reinterpret_cast<const DRAWITEMSTRUCT*>(data);
        if (flyout->suggesting) PaintSuggestion(*flyout, item);
        else PaintTile(*flyout, item);
        return TRUE;
    }
    case WM_CTLCOLORLISTBOX: {
        const auto dc = reinterpret_cast<HDC>(key);
        const auto rgb = Rgb(flyout->colors.surface);
        SetBkColor(dc, rgb);
        SetDCBrushColor(dc, rgb);
        return reinterpret_cast<LRESULT>(GetStockObject(DC_BRUSH));
    }
    case WM_COMMAND:
        if (!flyout->suggesting && data && HIWORD(key) == BN_CLICKED) {
            InvokeTile(*flyout, LOWORD(key) - firstTileId);
        }
        return 0;
    case WM_NOTIFY: {
        auto& notification = *reinterpret_cast<NMHDR*>(data);
        if (notification.hwndFrom != flyout->tooltips || notification.code != TTN_GETDISPINFOW) break;
        auto& tip = *reinterpret_cast<NMTTDISPINFOW*>(data);
        GetWindowTextW(reinterpret_cast<HWND>(notification.idFrom), tip.szText, static_cast<int>(std::size(tip.szText)));
        return 0;
    }
    case WM_MOUSEWHEEL:
        if (flyout->suggesting) break;
        flyout->wheelRemainder += GET_WHEEL_DELTA_WPARAM(key);
        ScrollTo(*flyout, flyout->topRow - flyout->wheelRemainder / WHEEL_DELTA);
        flyout->wheelRemainder %= WHEEL_DELTA;
        return 0;
    case WM_VSCROLL:
        if (!flyout->scrollbar || reinterpret_cast<HWND>(data) != flyout->scrollbar) break;
        switch (LOWORD(key)) {
        case SB_LINEUP: ScrollTo(*flyout, flyout->topRow - 1); break;
        case SB_LINEDOWN: ScrollTo(*flyout, flyout->topRow + 1); break;
        case SB_PAGEUP: ScrollTo(*flyout, flyout->topRow - flyout->pageRows); break;
        case SB_PAGEDOWN: ScrollTo(*flyout, flyout->topRow + flyout->pageRows); break;
        case SB_TOP: ScrollTo(*flyout, 0); break;
        case SB_BOTTOM: ScrollTo(*flyout, flyout->Rows()); break;
        case SB_THUMBPOSITION:
        case SB_THUMBTRACK: {
            SCROLLINFO scroll{sizeof(scroll), SIF_TRACKPOS};
            if (GetScrollInfo(flyout->scrollbar, SB_CTL, &scroll)) ScrollTo(*flyout, scroll.nTrackPos);
            break;
        }
        }
        return 0;
    case WM_ACTIVATE:
        if (LOWORD(key) == WA_INACTIVE) PostMessageW(window, dismiss, 0, 0);
        break;
    case dismiss:
        if (GetActiveWindow() == window) return 0;
        [[fallthrough]];
    case WM_CLOSE:
        DestroyWindow(window);
        return 0;
    case WM_PAINT: {
        PAINTSTRUCT paint{};
        const HDC dc = BeginPaint(window, &paint);
        PaintPanel(*flyout, dc);
        EndPaint(window, &paint);
        return 0;
    }
    case WM_PRINTCLIENT:
        PaintPanel(*flyout, reinterpret_cast<HDC>(key));
        return 0;
    case WM_ERASEBKGND:
        return 1;
    case WM_NCDESTROY:
        SetWindowLongPtrW(window, GWLP_USERDATA, 0);
        flyout->actions.closed(window);
        delete flyout;
        break;
    }
    return DefWindowProcW(window, message, key, data);
}

}

bool PositionFlyout(Flyout& flyout, HWND anchor)
{
    RECT button{}, ownerBounds{};
    const HWND owner = GetAncestor(anchor, GA_ROOT);
    if (!GetWindowRect(anchor, &button) || !GetWindowRect(owner, &ownerBounds)) return false;
    MONITORINFO monitor{sizeof(monitor)};
    if (!GetMonitorInfoW(MonitorFromRect(&ownerBounds, MONITOR_DEFAULTTONEAREST), &monitor)) return false;

    const auto work = monitor.rcWork;
    const LONG gap = flyout.Px(6);
    const LONG below = ownerBounds.bottom + gap;
    const LONG roomBelow = work.bottom - below;
    const LONG roomAbove = ownerBounds.top - gap - work.top;
    LONG width{}, height{};
    if (flyout.suggesting) {
        width = std::min(ownerBounds.right - ownerBounds.left, work.right - work.left);
        const int body = headerHeight + 4 + static_cast<int>(std::min<size_t>(5, flyout.visibleEngines.size())) * rowHeight;
        height = std::min<LONG>(flyout.Px(body + inset), work.bottom - work.top);
    } else {
        const int padding = flyout.Px(gridInset);
        const int gapX = flyout.Px(tileGap);
        const int edge = flyout.TileWidth();
        const int row = flyout.TileHeight();
        const int scrollbar = GetSystemMetricsForDpi(SM_CXVSCROLL, flyout.dpi);
        const int room = work.right - work.left;
        flyout.columns = std::clamp((room - 2 * padding - scrollbar + gapX) / (edge + gapX), 1, gridColumns);
        const int roomY = std::max(roomBelow, roomAbove);
        flyout.pageRows = std::min({maxGridRows, flyout.Rows(), std::max(1, (roomY - 2 * padding + gapX) / (row + gapX))});
        flyout.topRow = std::clamp(flyout.selectedTile / flyout.columns - flyout.pageRows + 1, 0, flyout.Rows() - flyout.pageRows);
        const bool overflow = flyout.Rows() > flyout.pageRows;
        width = std::min(room, 2 * padding + flyout.columns * edge + (flyout.columns - 1) * gapX + (overflow ? scrollbar : 0));
        height = std::min<LONG>(2 * padding + flyout.pageRows * row + (flyout.pageRows - 1) * gapX, work.bottom - work.top);
        if (overflow) {
            flyout.scrollbar = CreateWindowExW(0, L"SCROLLBAR", nullptr, WS_CHILD | WS_VISIBLE | SBS_VERT,
                0, 0, 0, 0, flyout.window, nullptr, GetModuleHandleW(nullptr), nullptr);
            if (!flyout.scrollbar) return false;
            SCROLLINFO scroll{sizeof(scroll), SIF_RANGE | SIF_PAGE | SIF_POS, 0, flyout.Rows() - 1,
                static_cast<UINT>(flyout.pageRows), flyout.topRow};
            SetScrollInfo(flyout.scrollbar, SB_CTL, &scroll, TRUE);
        }
    }
    const LONG above = ownerBounds.top - gap - height;
    const LONG preferred = roomBelow >= height || roomBelow >= roomAbove ? below : above;
    const LONG x = std::clamp(ownerBounds.right - width, work.left, work.right - width);
    const LONG y = std::clamp(preferred, work.top, work.bottom - height);
    return SetWindowPos(flyout.window, nullptr, x, y, width, height, SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER) != FALSE;
}

HWND CreateEngineFlyout(HWND anchor, const Application& app, ID2D1Factory& drawingFactory,
    IDWriteFactory& textFactory, IDWriteTextFormat& engineFont, EngineFlyoutActions actions, EngineId selected,
    const std::vector<const Engine*>* suggestions)
{
    const HINSTANCE instance = GetModuleHandleW(nullptr);
    WNDCLASSW windowClass{};
    windowClass.hInstance = instance;
    windowClass.lpfnWndProc = FlyoutProc;
    windowClass.lpszClassName = className;
    windowClass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    if (!RegisterClassW(&windowClass) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) return nullptr;

    RECT button{};
    if (!GetWindowRect(anchor, &button)) return nullptr;
    if (!app.findEngine(selected)) selected = app.preferences().engine;
    auto info = CreateInfo{app, drawingFactory, textFactory, engineFont, actions, std::move(selected), suggestions};
    const DWORD exStyle = WS_EX_TOOLWINDOW | (suggestions ? WS_EX_NOACTIVATE : WS_EX_CONTROLPARENT);
    const HWND window = CreateWindowExW(exStyle, className, Tr(Text::chooseEngine),
        WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN, button.left, button.top, 1, 1,
        GetAncestor(anchor, GA_ROOT), nullptr, instance, &info);
    if (!window) return nullptr;

    auto& flyout = *GetFlyout(window);
    const DWM_WINDOW_CORNER_PREFERENCE corners = DWMWCP_ROUND;
    DwmSetWindowAttribute(window, DWMWA_WINDOW_CORNER_PREFERENCE, &corners, sizeof(corners));
    constexpr COLORREF noBorder = 0xfffffffe;
    DwmSetWindowAttribute(window, DWMWA_BORDER_COLOR, &noBorder, sizeof(noBorder));
    if (!PositionFlyout(flyout, anchor)) {
        DestroyWindow(window);
        return nullptr;
    }
    return window;
}

void ShowEngineFlyout(HWND window)
{
    auto* flyout = GetFlyout(window);
    if (!flyout) return;
    ShowWindow(window, flyout->suggesting ? SW_SHOWNOACTIVATE : SW_SHOW);
    if (!flyout->suggesting) FocusTile(*flyout, flyout->selectedTile);
}

bool RefreshEngineSuggestions(HWND window, const std::vector<const Engine*>& suggestions)
{
    auto* flyout = GetFlyout(window);
    if (!flyout || !flyout->suggesting || suggestions.empty()) return false;
    flyout->visibleEngines.clear();
    for (const auto* engine : suggestions) flyout->visibleEngines.push_back(engine - flyout->app.engines().data());
    PopulateEngines(*flyout);
    return PositionFlyout(*flyout, GetWindow(window, GW_OWNER));
}

void RepositionEngineSuggestions(HWND window)
{
    if (auto* flyout = GetFlyout(window); flyout && flyout->suggesting) {
        if (!PositionFlyout(*flyout, GetWindow(window, GW_OWNER))) DestroyWindow(window);
    }
}

bool HandleEngineSuggestionKey(HWND window, WPARAM key)
{
    auto* flyout = GetFlyout(window);
    if (!flyout || !flyout->suggesting) return false;
    switch (key) {
    case VK_ESCAPE:
        DestroyWindow(window);
        return true;
    case VK_TAB:
        InvokeEngine(*flyout, ListBox_GetCurSel(flyout->engineList));
        return true;
    case VK_UP:
    case VK_DOWN: {
        const int count = ListBox_GetCount(flyout->engineList);
        if (count > 0) {
            const int current = ListBox_GetCurSel(flyout->engineList);
            const int next = current + (key == VK_UP ? -1 : 1);
            ListBox_SetCurSel(flyout->engineList, (next + count) % count);
        }
        return true;
    }
    }
    return false;
}
bool HandleEngineFlyoutMessage(HWND window, const MSG& message)
{
    if (!window || (message.hwnd != window && !IsChild(window, message.hwnd)) || message.message != WM_KEYDOWN) return false;
    auto* flyout = GetFlyout(window);
    if (!flyout || flyout->suggesting) return false;
    const int count = flyout->TileCount();
    const int columns = flyout->columns;
    const auto focused = std::ranges::find(flyout->tiles, message.hwnd);
    const int current = focused == flyout->tiles.end() ? flyout->topRow * columns
        : static_cast<int>(focused - flyout->tiles.begin());
    int next = current;
    switch (message.wParam) {
    case VK_ESCAPE:
        DestroyWindow(window);
        return true;
    case VK_TAB:
        next = (current + ((GetKeyState(VK_SHIFT) & 0x8000) ? count - 1 : 1)) % count;
        break;
    case VK_LEFT: next = std::max(0, current - 1); break;
    case VK_RIGHT: next = std::min(count - 1, current + 1); break;
    case VK_UP: if (current >= columns) next = current - columns; break;
    case VK_DOWN: if (current / columns + 1 < flyout->Rows()) next = std::min(count - 1, current + columns); break;
    case VK_HOME: next = 0; break;
    case VK_END: next = count - 1; break;
    case VK_RETURN:
    case VK_SPACE:
        InvokeTile(*flyout, current);
        return true;
    case VK_OEM_COMMA:
        if (!(GetKeyState(VK_CONTROL) & 0x8000)) return false;
        OpenEngineSettings(*flyout, false);
        return true;
    default:
        return false;
    }
    FocusTile(*flyout, next);
    return true;
}

}
