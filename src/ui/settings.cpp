#include "../platform/localization.h"
#include "settings.h"
#include "engine_icons.h"
#include "visual_style.h"
#include "../platform/browser.h"
#include "../platform/config.h"
#include "../platform/performance.h"
#include "../platform/windows_services.h"

#include <commctrl.h>
#include <commdlg.h>
#include <shellapi.h>
#include <d2d1.h>
#include <dwmapi.h>
#include <windowsx.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <filesystem>
#include <memory>
#include <optional>
#include <tuple>
#include <type_traits>
#include <utility>

namespace yisou::ui {
using i18n::Text;
using i18n::Tr;

namespace {
using Microsoft::WRL::ComPtr;

constexpr wchar_t className[] = L"YiSouSettings";
constexpr int sidebarWidth = 194;
constexpr int contentGutter = 26;
constexpr int navRowHeight = 42;
constexpr int firstCardTop = 116;
constexpr int secondCardTop = 196;
constexpr int previewCardTop = 276;
constexpr int engineMenuTop = 404;
constexpr int engineRowHeight = 52;
constexpr int editorCardTop = 90;
constexpr int nameFieldTop = 136;
constexpr int urlFieldTop = 207;
constexpr int browserRowHeight = 46;
constexpr int historyRowHeight = 104;
constexpr size_t historyPageSize = 24;
constexpr UINT_PTR historyFilterTimer = 1;
constexpr int historyFiltersTop = 120;
constexpr int historyListTop = 166;
enum Page { generalPage, appearancePage, enginePage, shortcutPage, browserPage, historyPage, configurationPage };
constexpr std::array pageNames{Text::general, Text::appearance, Text::searchEngines, Text::shortcuts,
    Text::browser, Text::searchHistory, Text::configuration};
constexpr std::array pageIcons{TablerIcon::settings, TablerIcon::palette, TablerIcon::search,
    TablerIcon::keyboard, TablerIcon::browser, TablerIcon::history, TablerIcon::fileTypography};

enum ControlId {
    navigation = 201, systemFont, localFont, resetFont, engineList, newEngine,
    engineName, engineUrl, chooseIconColor, chooseIconFile, saveEngine, cancelEngine,
    recordShortcut, saveShortcut, cancelShortcut, resetShortcut, selectBrowser, browserList,
    otherBrowser, browseBrowser, saveBrowser, cancelBrowser, browserLocation,
    engineAlias, editEngine, deleteEngine, moveEngineUp, moveEngineDown, defaultEngine, testEngine,
    historyList, toggleHistory, moreHistory, selectionEngineList, recordSelectionShortcut, clearSelectionShortcut,
    pinEngine, selectionMode, historyKeyword, historySource, startWithWindows, showOnLaunch, closeToTray,
    selectConfig, importConfig, exportConfig, importEngines, exportEngines,
    recordClipboardShortcut, clearClipboardShortcut, excludedAppList, applyExcludedApps, languageChoice, applyLanguage,
    engineMenuWithLabels, engineMenuIconsOnly
};

constexpr std::array generalOptions{
    std::tuple{startWithWindows, Text::startAtSignIn, Text::startAtSignInDetail},
    std::tuple{showOnLaunch, Text::showAtLaunch, Text::showAtLaunchDetail},
    std::tuple{closeToTray, Text::stayInTray, Text::stayInTrayDetail}
};
constexpr int generalCardTop = 116;
constexpr int generalCardStep = 76;
constexpr int languageTop = generalCardTop + static_cast<int>(generalOptions.size()) * generalCardStep;
constexpr std::array languages{Language::system, Language::simplifiedChinese, Language::english};
constexpr std::array languageLabels{Text::followSystemLanguage, Text::simplifiedChinese, Text::english};

enum class BrowserPanel { closed, list, path };

struct BrowserRow {
    std::wstring name;
    std::wstring detail;
    std::wstring path;
    std::shared_ptr<std::remove_pointer_t<HICON>> icon;
};

struct Colors {
    COLORREF surface = panelRail;
    COLORREF card = panelSurface;
    COLORREF border = panelBorder;
    COLORREF text = RGB(26, 30, 38);
    COLORREF secondary = RGB(84, 94, 111);
    COLORREF accent = panelAccent;
    COLORREF primary = panelAccent;
    COLORREF primaryText = RGB(255, 255, 255);
    COLORREF selection = panelSelection;
    COLORREF hover = panelHover;
    COLORREF button = RGB(255, 255, 255);
    COLORREF pressed = RGB(222, 231, 245);
};

struct SettingsWindow {
    HWND window = nullptr;
    HWND nav = nullptr;
    HWND title = nullptr;
    HWND section = nullptr;
    HWND fontHeading = nullptr;
    HWND fontName = nullptr;
    HWND fileHeading = nullptr;
    HWND fontPath = nullptr;
    HWND previewHeading = nullptr;
    HWND preview = nullptr;
    HWND systemButton = nullptr;
    HWND fileButton = nullptr;
    HWND resetButton = nullptr;
    HWND engines = nullptr;
    HWND addEngine = nullptr;
    HWND nameLabel = nullptr;
    HWND name = nullptr;
    HWND urlLabel = nullptr;
    HWND url = nullptr;
    HWND urlHint = nullptr;
    HWND iconLabel = nullptr;
    HWND iconColor = nullptr;
    HWND iconFile = nullptr;
    HWND save = nullptr;
    HWND cancel = nullptr;
    HWND status = nullptr;
    HWND shortcutTitle = nullptr;
    HWND shortcutHint = nullptr;
    HWND shortcutRecord = nullptr;
    HWND shortcutSave = nullptr;
    HWND shortcutCancel = nullptr;
    HWND shortcutReset = nullptr;
    HWND selectionTitle = nullptr;
    HWND selectionHint = nullptr;
    HWND selectionEngines = nullptr;
    HWND selectionMode = nullptr;
    HWND selectionRecord = nullptr;
    HWND selectionClear = nullptr;
    HWND browserSelect = nullptr;
    HWND browsers = nullptr;
    HWND browserOther = nullptr;
    HWND browserPath = nullptr;
    HWND browserBrowse = nullptr;
    HWND browserSave = nullptr;
    HWND browserCancel = nullptr;
    HWND aliasLabel = nullptr;
    HWND alias = nullptr;
    HWND engineEdit = nullptr;
    HWND engineDelete = nullptr;
    HWND engineUp = nullptr;
    HWND engineDown = nullptr;
    HWND engineDefault = nullptr;
    HWND enginePin = nullptr;
    HWND engineTest = nullptr;
    HWND history = nullptr;
    HWND historyToggle = nullptr;
    HWND historyMore = nullptr;
    HWND historyEmpty = nullptr;
    HWND historyKeyword = nullptr;
    HWND historySource = nullptr;
    std::array<HWND, generalOptions.size()> generalTitles{}, generalDetails{}, generalButtons{};
    HWND languageTitle = nullptr;
    HWND languageHint = nullptr;
    HWND languageCombo = nullptr;
    HWND languageApply = nullptr;
    HWND engineMenuTitle = nullptr;
    HWND engineMenuLabels = nullptr;
    HWND engineMenuIcons = nullptr;
    HWND configPathText = nullptr;
    HWND configHelp = nullptr;
    HWND configSelect = nullptr;
    HWND configImport = nullptr;
    HWND configExport = nullptr;
    HWND engineTransferTitle = nullptr;
    HWND engineImport = nullptr;
    HWND engineExport = nullptr;
    HWND clipboardTitle = nullptr;
    HWND clipboardHint = nullptr;
    HWND clipboardRecord = nullptr;
    HWND clipboardClear = nullptr;
    HWND excludedTitle = nullptr;
    HWND excludedApps = nullptr;
    HWND excludedSave = nullptr;
    HFONT headerFont = nullptr;
    HFONT bodyFont = nullptr;
    HFONT detailFont = nullptr;
    HFONT previewFont = nullptr;
    HBRUSH surfaceBrush = nullptr;
    HBRUSH cardBrush = nullptr;
    Colors colors;
    ComPtr<ID2D1Factory> factory;
    ComPtr<ID2D1DCRenderTarget> drawing;
    ComPtr<ID2D1SolidColorBrush> brush;
    UINT dpi = USER_DEFAULT_SCREEN_DPI;
    int page = 0;
    int scroll = 0;
    int hoveredPage = -1;
    HWND hoveredButton = nullptr;
    Application* application = nullptr;
    bool editing = false;
    HWND recording = nullptr;
    std::optional<Shortcut> shortcutDraft;
    platform::AltTaps altTaps;
    BrowserPanel browserPanel = BrowserPanel::closed;
    std::vector<BrowserRow> browserRows;
    std::vector<SearchRecord> historyRows;
    bool historyFilterPending = false;
    Engine draft;
    SettingsValues values;
    SettingsActions actions{};
};

SettingsWindow panel;

int Px(int value) { return MulDiv(value, panel.dpi, USER_DEFAULT_SCREEN_DPI); }

struct ContentBounds {
    int left;
    int width;
};

ContentBounds ContentArea(int clientWidth)
{
    const int available = clientWidth - sidebarWidth - contentGutter * 2;
    const int width = std::clamp(available, 0, 720);
    return {sidebarWidth + contentGutter + (available - width) / 2, width};
}

D2D1_COLOR_F Color(COLORREF color)
{
    return D2D1::ColorF(GetRValue(color) / 255.f, GetGValue(color) / 255.f, GetBValue(color) / 255.f);
}

bool BeginShapes(HDC dc, const RECT& bounds)
{
    if (IsRectEmpty(&bounds)) return false;
    if (!panel.drawing) {
        const auto options = D2D1::RenderTargetProperties(D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_IGNORE));
        if (FAILED(panel.factory->CreateDCRenderTarget(&options, panel.drawing.GetAddressOf()))) return false;
        if (FAILED(panel.drawing->CreateSolidColorBrush(Color(panel.colors.card), panel.brush.GetAddressOf()))) {
            panel.drawing.Reset();
            return false;
        }
    }
    if (FAILED(panel.drawing->BindDC(dc, &bounds))) return false;
    panel.drawing->SetDpi(static_cast<float>(panel.dpi), static_cast<float>(panel.dpi));
    panel.drawing->SetTransform(D2D1::Matrix3x2F::Identity());
    panel.drawing->BeginDraw();
    return true;
}

void EndShapes()
{
    if (FAILED(panel.drawing->EndDraw())) {
        panel.brush.Reset();
        panel.drawing.Reset();
    }
}

void RoundedFill(D2D1_RECT_F bounds, float radius, COLORREF color)
{
    panel.brush->SetColor(Color(color));
    panel.drawing->FillRoundedRectangle(D2D1::RoundedRect(bounds, radius, radius), panel.brush.Get());
}

void RoundedStroke(D2D1_RECT_F bounds, float radius, COLORREF color, float weight = 1)
{
    panel.brush->SetColor(Color(color));
    panel.drawing->DrawRoundedRectangle(D2D1::RoundedRect(bounds, radius, radius), panel.brush.Get(), weight);
}

void RefreshPalette()
{
    panel.colors = {};
    HIGHCONTRASTW contrast{sizeof(contrast)};
    if (SystemParametersInfoW(SPI_GETHIGHCONTRAST, sizeof(contrast), &contrast, 0)
        && (contrast.dwFlags & HCF_HIGHCONTRASTON)) {
        panel.colors.surface = GetSysColor(COLOR_WINDOW);
        panel.colors.card = panel.colors.surface;
        panel.colors.border = GetSysColor(COLOR_WINDOWTEXT);
        panel.colors.text = panel.colors.border;
        panel.colors.secondary = panel.colors.text;
        panel.colors.accent = GetSysColor(COLOR_HOTLIGHT);
        panel.colors.primary = GetSysColor(COLOR_HIGHLIGHT);
        panel.colors.primaryText = GetSysColor(COLOR_HIGHLIGHTTEXT);
        panel.colors.selection = panel.colors.surface;
        panel.colors.hover = panel.colors.surface;
        panel.colors.button = GetSysColor(COLOR_BTNFACE);
        panel.colors.pressed = panel.colors.button;
    }
    if (panel.surfaceBrush) DeleteObject(panel.surfaceBrush);
    if (panel.cardBrush) DeleteObject(panel.cardBrush);
    panel.surfaceBrush = CreateSolidBrush(panel.colors.surface);
    panel.cardBrush = CreateSolidBrush(panel.colors.card);
    DwmSetWindowAttribute(panel.window, DWMWA_CAPTION_COLOR, &panel.colors.surface, sizeof(COLORREF));
    DwmSetWindowAttribute(panel.window, DWMWA_TEXT_COLOR, &panel.colors.text, sizeof(COLORREF));
}

HWND Control(const wchar_t* kind, const wchar_t* label, DWORD style = 0, UINT id = 0)
{
    return CreateWindowExW(0, kind, label, WS_CHILD | style, 0, 0, 0, 0, panel.window,
        reinterpret_cast<HMENU>(static_cast<UINT_PTR>(id)), GetModuleHandleW(nullptr), nullptr);
}

void Place(HWND control, int x, int y, int width, int height)
{
    MoveWindow(control, Px(x), Px(y), std::max(0, Px(width)), Px(height), TRUE);
}

void PlaceSetting(HWND control, int x, int y, int width, int height)
{
    MoveWindow(control, Px(x), Px(y) - panel.scroll, std::max(0, Px(width)), Px(height), TRUE);
}

int TextWidth(HDC dc, std::wstring_view label)
{
    SIZE size{};
    if (!dc || !GetTextExtentPoint32W(dc, label.data(), static_cast<int>(label.size()), &size)) return 0;
    return MulDiv(size.cx, USER_DEFAULT_SCREEN_DPI, panel.dpi);
}

HFONT NewFont(LOGFONTW face, int size, LONG weight)
{
    face.lfHeight = -Px(size);
    face.lfWidth = 0;
    face.lfWeight = weight;
    face.lfQuality = CLEARTYPE_QUALITY;
    return CreateFontIndirectW(&face);
}

void DrawTextWithFont(HDC dc, const wchar_t* text, RECT bounds, HFONT font, COLORREF color, UINT format)
{
    const HGDIOBJ previousFont = font ? SelectObject(dc, font) : nullptr;
    const COLORREF previousColor = SetTextColor(dc, color);
    const int previousMode = SetBkMode(dc, TRANSPARENT);
    DrawTextW(dc, text, -1, &bounds, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | format);
    SetBkMode(dc, previousMode);
    SetTextColor(dc, previousColor);
    if (previousFont) SelectObject(dc, previousFont);
}

void RefreshFontPreview()
{
    SetWindowTextW(panel.fontName, panel.values.fontFace.lfFaceName);
    const auto path = panel.values.font.source == FontSource::file
        ? std::filesystem::path(panel.values.font.value).filename().wstring()
        : Tr(Text::supportedFonts);
    SetWindowTextW(panel.fontPath, path.c_str());
    if (const HFONT font = NewFont(panel.values.fontFace, 26, FW_NORMAL)) {
        SendMessageW(panel.preview, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
        if (panel.previewFont) DeleteObject(panel.previewFont);
        panel.previewFont = font;
    }
    EnableWindow(panel.resetButton, panel.values.font.source != FontSource::defaults);
}

void RefreshWindowFonts()
{
    LOGFONTW face{};
    NONCLIENTMETRICSW metrics{sizeof(metrics)};
    if (SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS, sizeof(metrics), &metrics, 0, panel.dpi)) {
        face = metrics.lfMessageFont;
    }
    const HFONT body = NewFont(face, 14, FW_NORMAL);
    if (body) {
        for (size_t i = 0; i < generalOptions.size(); ++i) {
            SendMessageW(panel.generalTitles[i], WM_SETFONT, reinterpret_cast<WPARAM>(body), TRUE);
            SendMessageW(panel.generalButtons[i], WM_SETFONT, reinterpret_cast<WPARAM>(body), TRUE);
        }
        for (HWND control : {panel.nav, panel.section, panel.fontHeading, panel.fileHeading,
            panel.previewHeading, panel.systemButton, panel.fileButton, panel.resetButton, panel.engines,
            panel.addEngine, panel.nameLabel, panel.name, panel.urlLabel, panel.url, panel.iconLabel,
            panel.iconColor, panel.iconFile, panel.save, panel.cancel, panel.shortcutTitle, panel.shortcutRecord,
            panel.shortcutSave, panel.shortcutCancel, panel.shortcutReset, panel.selectionTitle,
            panel.selectionEngines, panel.selectionMode, panel.selectionRecord, panel.selectionClear, panel.browserSelect, panel.browsers,
            panel.browserOther, panel.browserPath, panel.browserBrowse, panel.browserSave, panel.browserCancel,
            panel.aliasLabel, panel.alias, panel.engineEdit, panel.engineDelete, panel.engineUp, panel.engineDown,
            panel.engineDefault, panel.enginePin, panel.engineTest, panel.history, panel.historyToggle, panel.historyMore,
            panel.historyEmpty, panel.historyKeyword, panel.historySource, panel.configPathText,
            panel.configSelect, panel.configImport, panel.configExport, panel.engineTransferTitle, panel.engineImport, panel.engineExport,
            panel.clipboardTitle, panel.clipboardRecord, panel.clipboardClear, panel.excludedTitle, panel.excludedApps, panel.excludedSave,
            panel.languageTitle, panel.languageCombo, panel.languageApply, panel.engineMenuTitle, panel.engineMenuLabels, panel.engineMenuIcons}) {
            SendMessageW(control, WM_SETFONT, reinterpret_cast<WPARAM>(body), TRUE);
        }
        if (panel.bodyFont) DeleteObject(panel.bodyFont);
        panel.bodyFont = body;
    }
    if (const HFONT detail = NewFont(face, 12, FW_NORMAL)) {
        for (HWND control : panel.generalDetails) SendMessageW(control, WM_SETFONT, reinterpret_cast<WPARAM>(detail), TRUE);
        for (HWND control : {panel.fontName, panel.fontPath, panel.urlHint, panel.status, panel.shortcutHint, panel.selectionHint,
            panel.configHelp, panel.clipboardHint, panel.languageHint}) {
            SendMessageW(control, WM_SETFONT, reinterpret_cast<WPARAM>(detail), TRUE);
        }
        if (panel.detailFont) DeleteObject(panel.detailFont);
        panel.detailFont = detail;
    }
    if (const HFONT header = NewFont(face, 30, FW_SEMIBOLD)) {
        SendMessageW(panel.title, WM_SETFONT, reinterpret_cast<WPARAM>(header), TRUE);
        if (panel.headerFont) DeleteObject(panel.headerFont);
        panel.headerFont = header;
    }
    ListBox_SetItemHeight(panel.nav, 0, Px(navRowHeight));
    ListBox_SetItemHeight(panel.engines, 0, Px(engineRowHeight));
    ListBox_SetItemHeight(panel.browsers, 0, Px(browserRowHeight));
    ListBox_SetItemHeight(panel.history, 0, std::min(255, Px(historyRowHeight)));
    RefreshFontPreview();
}

void Layout()
{
    RECT bounds{};
    GetClientRect(panel.window, &bounds);
    SCROLLINFO scroll{sizeof(scroll), SIF_RANGE | SIF_PAGE | SIF_POS};
    const int contentHeight = panel.page == shortcutPage ? 675 : panel.page == appearancePage ? 528
        : panel.page == generalPage ? 465 : panel.page == configurationPage ? 420 : panel.editing ? 450 : 0;
    scroll.nMax = std::max(0, Px(contentHeight) - 1);
    scroll.nPage = bounds.bottom;
    scroll.nPos = panel.scroll;
    panel.scroll = SetScrollInfo(panel.window, SB_VERT, &scroll, TRUE);
    GetClientRect(panel.window, &bounds);

    const auto [left, contentWidth] = ContentArea(MulDiv(bounds.right, USER_DEFAULT_SCREEN_DPI, panel.dpi));
    const int height = MulDiv(bounds.bottom, USER_DEFAULT_SCREEN_DPI, panel.dpi);
    const HDC dc = GetDC(panel.window);
    const HGDIOBJ previousFont = dc && panel.bodyFont ? SelectObject(dc, panel.bodyFont) : nullptr;
    const auto buttonWidth = [&](Text text, int minimum) { return std::max(minimum, TextWidth(dc, Tr(text)) + 16); };
    const int navigationHeight = std::min(height - 48, ListBox_GetCount(panel.nav) * navRowHeight);
    Place(panel.nav, 16, 34, sidebarWidth - 24, std::max(0, navigationHeight));
    PlaceSetting(panel.title, left, 30, contentWidth, 48);
    const int addEngineWidth = buttonWidth(Text::addSearchEngine, 134);
    PlaceSetting(panel.section, left, 88, panel.page == enginePage ? contentWidth - addEngineWidth - 11 : contentWidth, 22);
    for (size_t i = 0; i < generalOptions.size(); ++i) {
        const int top = generalCardTop + static_cast<int>(i) * generalCardStep;
        PlaceSetting(panel.generalTitles[i], left + 16, top + 12, contentWidth - 115, 22);
        PlaceSetting(panel.generalDetails[i], left + 16, top + 34, contentWidth - 115, 19);
        PlaceSetting(panel.generalButtons[i], left + contentWidth - 88, top + 17, 72, 32);
    }
    int languageWidth = 134;
    for (const auto label : languageLabels) languageWidth = std::max(languageWidth, TextWidth(dc, Tr(label)) + 32);
    const int languageLeft = left + contentWidth - 103 - languageWidth;
    PlaceSetting(panel.languageTitle, left + 16, languageTop + 12, languageLeft - left - 24, 22);
    PlaceSetting(panel.languageHint, left + 16, languageTop + 34, languageLeft - left - 24, 19);
    PlaceSetting(panel.languageCombo, languageLeft, languageTop + 20, languageWidth, 116);
    PlaceSetting(panel.languageApply, left + contentWidth - 88, languageTop + 17, 72, 32);

    const int cardText = left + 50;
    const int fontActionWidth = buttonWidth(Text::chooseSystemFont, 123);
    const int cardTextWidth = contentWidth - fontActionWidth - 79;
    const int action = left + contentWidth - fontActionWidth - 16;
    PlaceSetting(panel.fontHeading, cardText, firstCardTop + 16, cardTextWidth, 23);
    PlaceSetting(panel.fontName, cardText, firstCardTop + 39, cardTextWidth, 20);
    PlaceSetting(panel.fileHeading, cardText, secondCardTop + 16, cardTextWidth, 23);
    PlaceSetting(panel.fontPath, cardText, secondCardTop + 39, cardTextWidth, 20);
    PlaceSetting(panel.systemButton, action, firstCardTop + 20, fontActionWidth, 32);
    PlaceSetting(panel.fileButton, action, secondCardTop + 20, fontActionWidth, 32);
    PlaceSetting(panel.previewHeading, cardText, previewCardTop + 17, cardTextWidth, 23);
    PlaceSetting(panel.resetButton, action, previewCardTop + 13, fontActionWidth, 32);
    PlaceSetting(panel.preview, left + 21, previewCardTop + 60, contentWidth - 42, 38);
    const int modeWidth = std::max(buttonWidth(Text::engineMenuWithLabels, 74), buttonWidth(Text::engineMenuIconsOnly, 74));
    const int modeLeft = left + contentWidth - 16 - 2 * modeWidth - 4;
    PlaceSetting(panel.engineMenuTitle, cardText, engineMenuTop + 21, modeLeft - cardText - 10, 24);
    PlaceSetting(panel.engineMenuLabels, modeLeft, engineMenuTop + 17, modeWidth, 32);
    PlaceSetting(panel.engineMenuIcons, modeLeft + modeWidth + 4, engineMenuTop + 17, modeWidth, 32);
    PlaceSetting(panel.addEngine, left + contentWidth - addEngineWidth, 81, addEngineWidth, 32);
    PlaceSetting(panel.engines, left + 7, 130, contentWidth - 14, std::max(0, height - 210));
    const int toolsY = height - 70;
    int toolX = left;
    for (const auto& [control, size] : {std::pair{panel.engineDefault, buttonWidth(Text::setDefault, 78)},
        {panel.enginePin, buttonWidth(Text::unpin, 60)}, {panel.engineEdit, buttonWidth(Text::edit, 42)},
        {panel.engineDelete, buttonWidth(Text::deleteItem, 51)}, {panel.engineUp, buttonWidth(Text::moveUp, 51)},
        {panel.engineDown, buttonWidth(Text::moveDown, 51)}}) {
        PlaceSetting(control, toolX, toolsY, size, 31);
        toolX += size + 6;
    }
    const int formLeft = left + 20;
    const int formWidth = contentWidth - 40;
    const int nameWidth = formWidth * 3 / 5;
    PlaceSetting(panel.nameLabel, formLeft, 110, nameWidth, 22);
    PlaceSetting(panel.name, formLeft + 10, nameFieldTop + 7, nameWidth - 20, 22);
    PlaceSetting(panel.aliasLabel, formLeft + nameWidth + 12, 110, formWidth - nameWidth - 12, 22);
    PlaceSetting(panel.alias, formLeft + nameWidth + 22, nameFieldTop + 7, formWidth - nameWidth - 32, 22);
    PlaceSetting(panel.urlLabel, formLeft, 181, formWidth, 22);
    PlaceSetting(panel.url, formLeft + 10, urlFieldTop + 7, formWidth - 20, 22);
    PlaceSetting(panel.urlHint, formLeft, 248, formWidth, 20);
    PlaceSetting(panel.iconLabel, formLeft, 280, formWidth, 22);
    PlaceSetting(panel.iconColor, formLeft + 59, 312, 112, 32);
    PlaceSetting(panel.iconFile, formLeft + 181, 312, 112, 32);
    PlaceSetting(panel.cancel, left + contentWidth - 192, 376, 89, 34);
    PlaceSetting(panel.save, left + contentWidth - 94, 376, 94, 34);
    PlaceSetting(panel.engineTest, left, 376, 94, 34);
    const int right = left + contentWidth;
    PlaceSetting(panel.configPathText, left + 16, 132, contentWidth - 32, 24);
    int configX = left + 16;
    for (const auto& [control, label] : {std::pair{panel.configSelect, Text::changeLocation},
        {panel.configImport, Text::importConfiguration}, {panel.configExport, Text::exportConfiguration}}) {
        const int width = buttonWidth(label, 102);
        PlaceSetting(control, configX, 170, width, 32);
        configX += width + 8;
    }
    PlaceSetting(panel.configHelp, left + 16, 211, contentWidth - 32, 32);
    PlaceSetting(panel.engineTransferTitle, left + 16, 276, contentWidth - 32, 24);
    const int importWidth = buttonWidth(Text::importSearchEngines, 102);
    PlaceSetting(panel.engineImport, left + 16, 312, importWidth, 32);
    PlaceSetting(panel.engineExport, left + 24 + importWidth, 312, buttonWidth(Text::exportSearchEngines, 102), 32);
    PlaceSetting(panel.shortcutTitle, left + 18, 130, contentWidth - 36, 23);
    PlaceSetting(panel.shortcutHint, left + 18, 154, contentWidth - 36, 20);
    PlaceSetting(panel.shortcutRecord, left + 18, 184, contentWidth - (panel.recording == panel.shortcutRecord ? 217 : 132), 34);
    PlaceSetting(panel.shortcutReset, right - 105, 184, 87, 34);
    PlaceSetting(panel.selectionTitle, left + 18, 262, contentWidth - 36, 23);
    PlaceSetting(panel.selectionHint, left + 18, 286, contentWidth - 36, 20);
    PlaceSetting(panel.selectionEngines, left + 18, 316, contentWidth - 179, 196);
    PlaceSetting(panel.selectionMode, right - 151, 316, 133, 110);
    PlaceSetting(panel.selectionRecord, left + 18, 360, contentWidth - (panel.recording == panel.selectionRecord ? 217 : 132), 34);
    PlaceSetting(panel.selectionClear, right - 105, 360, 87, 34);
    PlaceSetting(panel.clipboardTitle, left + 18, 435, contentWidth - 36, 23);
    PlaceSetting(panel.clipboardHint, left + 18, 459, contentWidth - 36, 20);
    PlaceSetting(panel.clipboardRecord, left + 18, 485, contentWidth - (panel.recording == panel.clipboardRecord ? 217 : 132), 34);
    PlaceSetting(panel.clipboardClear, right - 105, 485, 87, 34);
    PlaceSetting(panel.excludedTitle, left + 18, 555, contentWidth - 36, 22);
    PlaceSetting(panel.excludedApps, left + 26, 590, contentWidth - 150, 22);
    PlaceSetting(panel.excludedSave, right - 105, 583, 87, 34);
    const int recordingTop = panel.recording == panel.clipboardRecord ? 485 : panel.recording == panel.selectionRecord ? 360 : 184;
    PlaceSetting(panel.shortcutSave, right - 105, recordingTop, 87, 34);
    PlaceSetting(panel.shortcutCancel, right - 190, recordingTop, 76, 34);
    PlaceSetting(panel.browserSelect, left, 116, contentWidth, 60);
    const int browserBottom = std::min(height - 77, 390);
    PlaceSetting(panel.browsers, left + 6, 186, contentWidth - 12, std::max(0, browserBottom - 186));
    PlaceSetting(panel.browserOther, left, std::max(187, browserBottom + 5), buttonWidth(Text::chooseAnotherBrowser, 163), 31);
    PlaceSetting(panel.browserPath, left + 16, 200, contentWidth - 124, 23);
    PlaceSetting(panel.browserBrowse, right - 97, 193, 88, 34);
    PlaceSetting(panel.browserSave, right - 97, 242, 88, 32);
    PlaceSetting(panel.browserCancel, right - 182, 242, 75, 32);
    PlaceSetting(panel.historyToggle, right - 134, 81, 134, 32);
    const int filterSplit = left + contentWidth * 3 / 5;
    PlaceSetting(panel.historyKeyword, left + 10, historyFiltersTop + 7, filterSplit - left - 24, 22);
    PlaceSetting(panel.historySource, filterSplit + 15, historyFiltersTop + 7, right - filterSplit - 25, 22);
    PlaceSetting(panel.history, left, historyListTop, contentWidth, std::max(0, height - historyListTop - 74));
    PlaceSetting(panel.historyEmpty, left, historyListTop + 11, contentWidth, 25);
    PlaceSetting(panel.historyMore, left, height - 70, 120, 31);
    if (panel.page == historyPage) PlaceSetting(panel.section, left, 88, contentWidth - 145, 22);
    const int statusTop = panel.page == appearancePage ? 484 : panel.editing ? 419
        : panel.page == shortcutPage ? 640 : panel.page == generalPage ? 425 : panel.page == configurationPage ? 375 : height - 30;
    PlaceSetting(panel.status, left, statusTop, contentWidth, 32);
    if (previousFont) SelectObject(dc, previousFont);
    if (dc) ReleaseDC(panel.window, dc);
    InvalidateRect(panel.window, nullptr, TRUE);
}

void SelectEngine()
{
    const auto& engines = panel.application->engines();
    const auto selected = std::ranges::find(engines, panel.application->preferences().engine, &Engine::id);
    ListBox_SetCurSel(panel.engines, static_cast<int>(selected - engines.begin()));
}

const Engine* SelectedEngine()
{
    const auto index = ListBox_GetCurSel(panel.engines);
    const auto& engines = panel.application->engines();
    return index >= 0 && static_cast<size_t>(index) < engines.size() ? &engines[index] : nullptr;
}

void UpdateEngineControls()
{
    const auto* selected = SelectedEngine();
    const auto& engines = panel.application->engines();
    const bool custom = selected && selected->group == EngineGroup::custom;
    EnableWindow(panel.engineDefault, selected && selected->id != panel.application->preferences().engine);
    EnableWindow(panel.enginePin, selected != nullptr);
    SetWindowTextW(panel.enginePin, selected && panel.application->isPinned(selected->id) ? Tr(Text::unpin) : Tr(Text::pin));
    EnableWindow(panel.engineEdit, custom);
    EnableWindow(panel.engineDelete, custom);
    EnableWindow(panel.engineUp, custom && selected != engines.data() && (selected - 1)->group == EngineGroup::custom);
    EnableWindow(panel.engineDown, custom && selected != &engines.back() && (selected + 1)->group == EngineGroup::custom);
}

void RefreshEngineList(std::wstring_view preferred = {})
{
    ListBox_ResetContent(panel.engines);
    const auto& engines = panel.application->engines();
    int selected = -1;
    for (const auto& engine : engines) {
        const int row = ListBox_AddString(panel.engines, i18n::EngineName(engine.id, engine.name.c_str()));
        if (engine.id == (preferred.empty() ? std::wstring_view(panel.application->preferences().engine) : preferred)) selected = row;
    }
    if (selected >= 0) ListBox_SetCurSel(panel.engines, selected);
    else SelectEngine();
    UpdateEngineControls();
}

std::wstring ReadControl(HWND control);

void RefreshHistory(bool more = false)
{
    KillTimer(panel.window, historyFilterTimer);
    panel.historyFilterPending = false;
    if (!more) {
        panel.historyRows.clear();
        ListBox_ResetContent(panel.history);
    }
    const std::wstring_view before = panel.historyRows.empty() ? std::wstring_view{} : panel.historyRows.back().id;
    const auto keyword = ReadControl(panel.historyKeyword);
    const auto source = ReadControl(panel.historySource);
    auto rows = platform::LoadHistory(before, historyPageSize + 1, keyword, source);
    const bool hasMore = rows.size() > historyPageSize;
    if (hasMore) rows.pop_back();
    for (auto& row : rows) {
        if (ListBox_AddString(panel.history, row.query.c_str()) < 0) break;
        panel.historyRows.push_back(std::move(row));
    }
    const bool empty = panel.historyRows.empty();
    ShowWindow(panel.history, empty ? SW_HIDE : SW_SHOW);
    ShowWindow(panel.historyMore, hasMore ? SW_SHOW : SW_HIDE);
    ShowWindow(panel.historyEmpty, empty ? SW_SHOW : SW_HIDE);
    SetWindowTextW(panel.historyEmpty, keyword.empty() && source.empty() ? Tr(Text::noHistory) : Tr(Text::noMatchingHistory));
    SetWindowTextW(panel.historyToggle, panel.application->preferences().recordHistory ? Tr(Text::historyOn) : Tr(Text::historyOff));
    InvalidateRect(panel.historyToggle, nullptr, FALSE);
}

bool SameBrowser(const std::wstring& left, const std::wstring& right)
{
    return CompareStringOrdinal(left.c_str(), -1, right.c_str(), -1, TRUE) == CSTR_EQUAL;
}

std::shared_ptr<std::remove_pointer_t<HICON>> BrowserIcon(const std::wstring& path)
{
    HICON icon = nullptr;
    if (!path.empty()) ExtractIconExW(path.c_str(), 0, nullptr, &icon, 1);
    return {icon, [](HICON handle) { if (handle) DestroyIcon(handle); }};
}

void RefreshBrowsers()
{
    YISOU_PERF_MEMORY_SCOPE("refresh_browsers");
    const auto& selected = panel.application->preferences().browser;
    const auto defaultPath = platform::DefaultBrowserPath();
    const auto defaultBrowser = platform::DescribeBrowser(defaultPath);
    panel.browserRows.clear();
    panel.browserRows.push_back({Tr(Text::useDefaultBrowser), defaultBrowser.name, {}, BrowserIcon(defaultPath)});
    for (auto& browser : platform::DiscoverBrowsers()) {
        panel.browserRows.push_back({std::move(browser.name), browser.path, browser.path, BrowserIcon(browser.path)});
    }
    const auto known = std::ranges::find_if(panel.browserRows, [&](const BrowserRow& row) { return SameBrowser(row.path, selected); });
    if (known == panel.browserRows.end()) {
        const auto path = platform::BrowserPath(selected);
        const auto browser = platform::DescribeBrowser(selected);
        panel.browserRows.push_back({browser.name, path.empty() ? Tr(Text::fileMissing) : selected, selected, BrowserIcon(path)});
    }
    ListBox_ResetContent(panel.browsers);
    int index = 0;
    for (const auto& row : panel.browserRows) {
        const int added = ListBox_AddString(panel.browsers, row.name.c_str());
        if (SameBrowser(row.path, selected) && added >= 0) index = added;
    }
    ListBox_SetCurSel(panel.browsers, index);
    SetWindowTextW(panel.browserSelect, panel.browserRows[index].name.c_str());
    InvalidateRect(panel.browserSelect, nullptr, FALSE);
}

void SetBrowserPanel(BrowserPanel mode)
{
    panel.browserPanel = mode;
    for (HWND control : {panel.browsers, panel.browserOther}) {
        ShowWindow(control, panel.page == browserPage && mode == BrowserPanel::list ? SW_SHOW : SW_HIDE);
    }
    for (HWND control : {panel.browserPath, panel.browserBrowse, panel.browserSave, panel.browserCancel}) {
        ShowWindow(control, panel.page == browserPage && mode == BrowserPanel::path ? SW_SHOW : SW_HIDE);
    }
    InvalidateRect(panel.window, nullptr, TRUE);
    InvalidateRect(panel.browserSelect, nullptr, FALSE);
}

void SetShortcutRecording(HWND control = nullptr)
{
    if (panel.recording == control) return;
    panel.recording = control;
    panel.altTaps.reset();
    panel.shortcutDraft.reset();
    if (panel.actions.recordShortcut) panel.actions.recordShortcut(control != nullptr);
    SetWindowTextW(panel.shortcutRecord, control == panel.shortcutRecord ? Tr(Text::pressShortcut) : Tr(Text::changeShortcut));
    if (control == panel.selectionRecord) SetWindowTextW(panel.selectionRecord, Tr(Text::pressKeyCombination));
    else SetWindowTextW(panel.selectionRecord, panel.application->preferences().selectionShortcut
        ? Tr(Text::changeSelectionShortcut) : Tr(Text::recordSelectionShortcut));
    SetWindowTextW(panel.clipboardRecord, control == panel.clipboardRecord ? Tr(Text::pressKeyCombination) :
        panel.application->preferences().clipboardShortcut ? Tr(Text::changeClipboardShortcut) : Tr(Text::recordClipboardShortcut));
    InvalidateRect(panel.shortcutRecord, nullptr, FALSE);
    InvalidateRect(panel.selectionRecord, nullptr, FALSE);
    InvalidateRect(panel.clipboardRecord, nullptr, FALSE);
    ShowWindow(panel.shortcutSave, control ? SW_SHOW : SW_HIDE);
    ShowWindow(panel.shortcutCancel, control ? SW_SHOW : SW_HIDE);
    ShowWindow(panel.shortcutReset, !control && panel.page == shortcutPage ? SW_SHOW : SW_HIDE);
    ShowWindow(panel.selectionClear, !control && panel.page == shortcutPage ? SW_SHOW : SW_HIDE);
    ShowWindow(panel.clipboardClear, !control && panel.page == shortcutPage ? SW_SHOW : SW_HIDE);
    EnableWindow(panel.selectionEngines, !control);
    EnableWindow(panel.selectionMode, !control);
    EnableWindow(panel.shortcutSave, FALSE);
    Layout();
    if (control) SetFocus(control);
}

void UpdateShortcutHint()
{
    const bool active = !panel.actions.shortcutActive || panel.actions.shortcutActive();
    SetWindowTextW(panel.shortcutHint, active ? Tr(Text::mainShortcutHint) : Tr(Text::mainShortcutUnavailable));
    const auto& binding = panel.application->preferences().selectionShortcut;
    const bool selectionActive = !binding || !panel.actions.selectionShortcutActive || panel.actions.selectionShortcutActive();
    SetWindowTextW(panel.selectionHint, selectionActive ? Tr(Text::selectionShortcutHint)
        : Tr(Text::selectionShortcutUnavailable));
    EnableWindow(panel.selectionClear, binding.has_value());
    if (panel.recording != panel.selectionRecord) {
        SetWindowTextW(panel.selectionRecord, binding ? Tr(Text::changeSelectionShortcut) : Tr(Text::recordSelectionShortcut));
    }
    InvalidateRect(panel.selectionRecord, nullptr, FALSE);
    const auto& clipboard = panel.application->preferences().clipboardShortcut;
    const bool clipboardActive = !clipboard || !panel.actions.clipboardShortcutActive || panel.actions.clipboardShortcutActive();
    SetWindowTextW(panel.clipboardHint, clipboardActive ? Tr(Text::clipboardShortcutHint)
        : Tr(Text::clipboardShortcutUnavailable));
    EnableWindow(panel.clipboardClear, clipboard.has_value());
    InvalidateRect(panel.clipboardRecord, nullptr, FALSE);
}

void RefreshSelectionEngines()
{
    const auto& binding = panel.application->preferences().selectionShortcut;
    const auto& selected = binding ? binding->engine : panel.application->preferences().engine;
    SendMessageW(panel.selectionMode, CB_SETCURSEL, !binding || binding->preview ? 1 : 0, 0);
    SendMessageW(panel.selectionEngines, CB_RESETCONTENT, 0, 0);
    const auto& engines = panel.application->engines();
    for (size_t index = 0; index < engines.size(); ++index) {
        const auto& engine = engines[index];
        std::wstring label = i18n::EngineName(engine.id, engine.name.c_str());
        if (!engine.alias.empty()) label += L"  #" + engine.alias;
        const auto row = SendMessageW(panel.selectionEngines, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(label.c_str()));
        if (row < 0) continue;
        SendMessageW(panel.selectionEngines, CB_SETITEMDATA, row, index);
        if (engine.id == selected) SendMessageW(panel.selectionEngines, CB_SETCURSEL, row, 0);
    }
}

const Engine* SelectedShortcutEngine()
{
    const auto row = SendMessageW(panel.selectionEngines, CB_GETCURSEL, 0, 0);
    const auto index = static_cast<size_t>(SendMessageW(panel.selectionEngines, CB_GETITEMDATA, row, 0));
    const auto& engines = panel.application->engines();
    return index < engines.size() ? &engines[index] : nullptr;
}

std::vector<std::wstring> ShortcutCaps(Shortcut shortcut)
{
    if (!shortcut.key) return {Tr(Text::leftAlt), L"× 2"};
    std::vector<std::wstring> caps;
    for (const auto& [flag, name] : {std::pair{shortcutControl, L"Ctrl"}, {shortcutShift, L"Shift"},
        {shortcutAlt, L"Alt"}, {shortcutWin, L"Win"}}) {
        if (shortcut.modifiers & flag) caps.emplace_back(name);
    }
    caps.push_back(platform::ShortcutKeyName(shortcut.key));
    return caps;
}

void PreviewShortcut(Shortcut shortcut)
{
    if (panel.recording != panel.shortcutRecord && !shortcut.key) return;
    panel.shortcutDraft = shortcut;
    std::wstring label;
    for (const auto& cap : ShortcutCaps(shortcut)) {
        if (!label.empty()) label += L" ";
        label += cap;
    }
    SetWindowTextW(panel.recording, label.c_str());
    SetWindowTextW(panel.status, L"");
    EnableWindow(panel.shortcutSave, TRUE);
    InvalidateRect(panel.recording, nullptr, FALSE);
}

bool ShowShortcutResult(platform::ShortcutResult result)
{
    switch (result) {
    case platform::ShortcutResult::applied:
        UpdateShortcutHint();
        SetWindowTextW(panel.status, Tr(Text::shortcutApplied));
        return true;
    case platform::ShortcutResult::unavailable:
        SetWindowTextW(panel.status, Tr(Text::cannotRegisterShortcut));
        break;
    case platform::ShortcutResult::saveFailed:
        SetWindowTextW(panel.status, Tr(Text::cannotSaveShortcut));
        break;
    }
    return false;
}

void ApplyShortcut(Shortcut shortcut)
{
    const auto result = panel.actions.changeShortcut ? panel.actions.changeShortcut(shortcut) : platform::ShortcutResult::unavailable;
    if (ShowShortcutResult(result)) {
        SetShortcutRecording();
        InvalidateRect(panel.shortcutRecord, nullptr, FALSE);
        SetFocus(panel.shortcutRecord);
    }
}

bool ApplySelectionBinding(const std::optional<SelectionShortcut>& binding)
{
    const auto result = panel.actions.changeSelectionShortcut ? panel.actions.changeSelectionShortcut(binding)
        : platform::ShortcutResult::unavailable;
    if (!ShowShortcutResult(result)) return false;
    if (!binding) SetWindowTextW(panel.status, Tr(Text::selectionShortcutCleared));
    return true;
}

void ApplyClipboardBinding(const std::optional<Shortcut>& binding)
{
    const auto result = panel.actions.changeClipboardShortcut ? panel.actions.changeClipboardShortcut(binding)
        : platform::ShortcutResult::unavailable;
    if (!ShowShortcutResult(result)) return;
    SetShortcutRecording();
    SetFocus(panel.clipboardRecord);
    if (!binding) SetWindowTextW(panel.status, Tr(Text::clipboardShortcutCleared));
}

void RefreshExcludedApps()
{
    std::wstring text;
    for (const auto& app : panel.application->preferences().doubleAltExcludedApps) {
        if (!text.empty()) text += L", ";
        text += app;
    }
    SetWindowTextW(panel.excludedApps, text.c_str());
}

void SaveExcludedApps()
{
    const auto text = ReadControl(panel.excludedApps);
    std::wstring_view remaining(text);
    std::vector<std::wstring> apps;
    while (!remaining.empty()) {
        const auto separator = remaining.find_first_of(L",，");
        const auto part = remaining.substr(0, separator);
        const auto first = part.find_first_not_of(L" \t");
        if (first != part.npos) {
            const auto name = part.substr(first, part.find_last_not_of(L" \t") - first + 1);
            if (!IsExecutableName(name)) {
                SetWindowTextW(panel.status, Tr(Text::enterExecutableName));
                return;
            }
            if (std::ranges::none_of(apps, [&](const auto& item) {
                return CompareStringOrdinal(item.c_str(), -1, name.data(), static_cast<int>(name.size()), TRUE) == CSTR_EQUAL;
            })) apps.emplace_back(name);
        }
        if (separator == remaining.npos) break;
        remaining.remove_prefix(separator + 1);
    }
    if (panel.application->changeExcludedApps(std::move(apps))) {
        RefreshExcludedApps();
        SetWindowTextW(panel.status, Tr(Text::excludedAppsSaved));
    } else SetWindowTextW(panel.status, Tr(Text::cannotSaveExcludedApps));
}

bool HandleShortcutRecording(const MSG& message)
{
    if (!panel.recording || message.hwnd != panel.recording) return false;
    if (message.message != WM_KEYDOWN && message.message != WM_KEYUP
        && message.message != WM_SYSKEYDOWN && message.message != WM_SYSKEYUP) return false;
    const auto key = static_cast<unsigned>(message.wParam);
    const auto modifiers = platform::ShortcutModifiers();
    if ((key == VK_TAB && !(modifiers & ~shortcutShift)) || key == VK_ESCAPE) return false;
    const bool down = message.message == WM_KEYDOWN || message.message == WM_SYSKEYDOWN;
    const bool alt = key == VK_MENU || key == VK_LMENU || key == VK_RMENU;
    const unsigned side = alt ? ((message.lParam & (1L << 24)) ? VK_RMENU : VK_LMENU) : key;
    if (panel.recording == panel.shortcutRecord && panel.altTaps.key(side, down, message.time, (modifiers & ~shortcutAlt) != 0)) {
        PreviewShortcut({});
        return true;
    }
    if (down && !(message.lParam & (1L << 30)) && platform::IsShortcut({modifiers, key})) PreviewShortcut({modifiers, key});
    return true;
}

void SaveBrowserSelection(std::wstring path, bool manual)
{
    if (manual || !path.empty()) {
        path = platform::BrowserPath(std::move(path));
        if (path.empty()) {
            SetWindowTextW(panel.status, Tr(Text::browserFileMissing));
            if (manual) SetFocus(panel.browserPath);
            return;
        }
    }
    if (!panel.application->changeBrowser(path)) {
        SetWindowTextW(panel.status, Tr(Text::cannotSaveBrowser));
        return;
    }
    RefreshBrowsers();
    SetBrowserPanel(BrowserPanel::closed);
    SetWindowTextW(panel.status, Tr(Text::browserApplied));
    SetFocus(panel.browserSelect);
}

void ApplyBrowserRow(int index)
{
    if (index >= 0 && static_cast<size_t>(index) < panel.browserRows.size()) {
        SaveBrowserSelection(panel.browserRows[index].path, false);
    }
}

void BrowseForBrowser()
{
    std::wstring path(32768, L'\0');
    OPENFILENAMEW dialog{sizeof(dialog)};
    dialog.hwndOwner = panel.window;
    dialog.lpstrFile = path.data();
    dialog.nMaxFile = static_cast<DWORD>(path.size());
    dialog.lpstrFilter = Tr(Text::executableFilter);
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    if (GetOpenFileNameW(&dialog)) SetWindowTextW(panel.browserPath, path.c_str());
}

bool GeneralOptionEnabled(ControlId option)
{
    const auto& preferences = panel.application->preferences();
    if (option == startWithWindows) return preferences.startWithWindows;
    if (option == showOnLaunch) return preferences.windowBehavior.showOnManualLaunch;
    return preferences.windowBehavior.closeToTray;
}

void UpdateGeneralOption(size_t index)
{
    const auto id = std::get<0>(generalOptions[index]);
    const auto label = i18n::Format(Text::optionWithState,
        {Tr(std::get<1>(generalOptions[index])), Tr(GeneralOptionEnabled(id) ? Text::on : Text::off)});
    SetWindowTextW(panel.generalButtons[index], label.c_str());
    InvalidateRect(panel.generalButtons[index], nullptr, FALSE);
}

void UpdateLanguageChoice()
{
    const auto choice = SendMessageW(panel.languageCombo, CB_GETCURSEL, 0, 0);
    const bool valid = choice >= 0 && static_cast<size_t>(choice) < languages.size();
    EnableWindow(panel.languageApply, valid && panel.actions.restartForLanguage
        && languages[static_cast<size_t>(choice)] != panel.application->preferences().language);
}

void ApplyLanguageChoice()
{
    const auto choice = SendMessageW(panel.languageCombo, CB_GETCURSEL, 0, 0);
    if (!IsWindowEnabled(panel.languageApply) || choice < 0 || static_cast<size_t>(choice) >= languages.size()) return;
    if (platform::config::SaveLanguage(languages[static_cast<size_t>(choice)])) {
        EnableWindow(panel.languageApply, FALSE);
        panel.actions.restartForLanguage(GetWindow(panel.window, GW_OWNER));
    } else SetWindowTextW(panel.status, Tr(Text::cannotSaveSetting));
}

void ToggleGeneralOption(ControlId option)
{
    const bool enabled = !GeneralOptionEnabled(option);
    auto behavior = panel.application->preferences().windowBehavior;
    if (option == showOnLaunch) behavior.showOnManualLaunch = enabled;
    if (option == closeToTray) behavior.closeToTray = enabled;
    const bool saved = option == startWithWindows ? panel.application->changeStartWithWindows(enabled)
        : panel.application->changeWindowBehavior(behavior);
    SetWindowTextW(panel.status, saved ? L"" : Tr(Text::cannotSaveSetting));
    for (size_t i = 0; i < generalOptions.size(); ++i) UpdateGeneralOption(i);
}

std::filesystem::path PickIni(const wchar_t* title, bool open, bool overwrite, const wchar_t* suggestion)
{
    std::wstring path(32768, L'\0');
    path.replace(0, std::wcslen(suggestion), suggestion);
    OPENFILENAMEW dialog{sizeof(dialog)};
    dialog.hwndOwner = panel.window;
    dialog.lpstrTitle = title;
    dialog.lpstrFilter = Tr(Text::iniFilter);
    dialog.lpstrDefExt = L"ini";
    dialog.lpstrFile = path.data();
    dialog.nMaxFile = static_cast<DWORD>(path.size());
    dialog.Flags = OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR
        | (open ? OFN_FILEMUSTEXIST : overwrite ? OFN_OVERWRITEPROMPT : 0);
    if (!(open ? GetOpenFileNameW(&dialog) : GetSaveFileNameW(&dialog))) return {};
    return std::filesystem::path(path.c_str());
}

void ManageConfiguration(UINT command)
{
    using namespace platform::config;
    if (!Active() || !panel.actions.restartForConfiguration) return;
    if (command == exportConfig || command == exportEngines) {
        const bool enginesOnly = command == exportEngines;
        const auto path = PickIni(enginesOnly ? Tr(Text::exportSearchEnginesTitle) : Tr(Text::exportConfigurationTitle), false, true,
            enginesOnly ? L"engines.ini" : L"settings.ini");
        if (!path.empty()) {
            const bool saved = enginesOnly ? ExportEngines(path) : Export(path);
            SetWindowTextW(panel.status, Describe(saved ? Result::ok : Result::ioError));
        }
        return;
    }
    const bool importing = command == importConfig || command == importEngines;
    const auto path = importing ? PickIni(command == importEngines ? Tr(Text::importSearchEnginesTitle) : Tr(Text::importConfigurationTitle), true, false, L"")
        : PickIni(Tr(Text::chooseConfigurationTitle), false, false, L"settings.ini");
    if (path.empty()) return;
    const auto result = command == importConfig ? Import(path) : command == importEngines ? ImportEngines(path) : ChooseLocation(path);
    if (result == Result::ok) panel.actions.restartForConfiguration(GetWindow(panel.window, GW_OWNER));
    else SetWindowTextW(panel.status, Describe(result));
}

void ShowPage(int page, bool editing = false)
{
    panel.page = page;
    if (panel.actions.scheduleGraphicsCleanup) panel.actions.scheduleGraphicsCleanup(GetWindow(panel.window, GW_OWNER));
    SetShortcutRecording();
    panel.editing = editing;
    panel.scroll = 0;
    const bool appearance = page == appearancePage;
    SetWindowTextW(panel.title, Tr(editing ? (panel.draft.id.empty() ? Text::addSearchEngine : Text::editSearchEngine) : pageNames[page]));
    constexpr std::array sections{Text::startupAndBackground, Text::searchAppearance, Text::defaultEngineAndAliases, Text::globalShortcuts,
        Text::resultsBrowser, Text::localSearchHistory, Text::currentConfigLocation};
    SetWindowTextW(panel.section, Tr(sections[page]));
    for (const auto control : {panel.languageTitle, panel.languageHint, panel.languageCombo, panel.languageApply}) {
        ShowWindow(control, page == generalPage ? SW_SHOW : SW_HIDE);
    }
    if (page == generalPage) {
        const auto selected = std::ranges::find(languages, panel.application->preferences().language);
        SendMessageW(panel.languageCombo, CB_SETCURSEL, static_cast<WPARAM>(selected - languages.begin()), 0);
        UpdateLanguageChoice();
    }
    for (const auto control : {panel.configPathText, panel.configSelect, panel.configImport, panel.configExport, panel.configHelp,
        panel.engineTransferTitle, panel.engineImport, panel.engineExport}) {
        ShowWindow(control, page == configurationPage ? SW_SHOW : SW_HIDE);
    }
    if (page == configurationPage) SetWindowTextW(panel.configPathText, platform::config::ActivePath().c_str());
    for (size_t i = 0; i < generalOptions.size(); ++i) {
        for (HWND control : {panel.generalTitles[i], panel.generalDetails[i], panel.generalButtons[i]}) {
            ShowWindow(control, page == generalPage ? SW_SHOW : SW_HIDE);
        }
        if (page == generalPage) UpdateGeneralOption(i);
    }
    for (HWND control : {panel.fontHeading, panel.fontName, panel.fileHeading, panel.fontPath,
        panel.previewHeading, panel.preview, panel.systemButton, panel.fileButton, panel.resetButton,
        panel.engineMenuTitle, panel.engineMenuLabels, panel.engineMenuIcons}) {
        ShowWindow(control, appearance ? SW_SHOW : SW_HIDE);
    }
    for (HWND control : {panel.engines, panel.addEngine, panel.engineEdit, panel.engineDelete,
        panel.engineUp, panel.engineDown, panel.engineDefault, panel.enginePin}) {
        ShowWindow(control, page == enginePage && !editing ? SW_SHOW : SW_HIDE);
    }
    for (HWND control : {panel.shortcutTitle, panel.shortcutHint, panel.shortcutRecord, panel.shortcutReset,
        panel.selectionTitle, panel.selectionHint, panel.selectionEngines, panel.selectionMode, panel.selectionRecord,
        panel.selectionClear, panel.clipboardTitle, panel.clipboardHint, panel.clipboardRecord, panel.clipboardClear,
        panel.excludedTitle, panel.excludedApps, panel.excludedSave}) {
        ShowWindow(control, page == shortcutPage ? SW_SHOW : SW_HIDE);
    }
    if (page == shortcutPage) {
        UpdateShortcutHint();
        RefreshSelectionEngines();
        RefreshExcludedApps();
    }
    if (page == browserPage) RefreshBrowsers();
    ShowWindow(panel.browserSelect, page == browserPage ? SW_SHOW : SW_HIDE);
    SetBrowserPanel(BrowserPanel::closed);
    for (HWND control : {panel.nameLabel, panel.name, panel.urlLabel, panel.url, panel.urlHint,
        panel.iconLabel, panel.iconColor, panel.iconFile, panel.save, panel.cancel, panel.aliasLabel, panel.alias, panel.engineTest}) {
        ShowWindow(control, editing ? SW_SHOW : SW_HIDE);
    }
    ShowWindow(panel.historyToggle, page == historyPage ? SW_SHOW : SW_HIDE);
    ShowWindow(panel.historyKeyword, page == historyPage ? SW_SHOW : SW_HIDE);
    ShowWindow(panel.historySource, page == historyPage ? SW_SHOW : SW_HIDE);
    if (page == historyPage) RefreshHistory();
    else {
        KillTimer(panel.window, historyFilterTimer);
        panel.historyFilterPending = false;
        for (HWND control : {panel.history, panel.historyEmpty, panel.historyMore}) ShowWindow(control, SW_HIDE);
        panel.historyRows.clear();
        ListBox_ResetContent(panel.history);
    }
    ShowWindow(panel.section, editing ? SW_HIDE : SW_SHOW);
    SetWindowTextW(panel.status, L"");
    Layout();
}

void StartEngineEditor(const Engine* engine = nullptr)
{
    panel.draft = engine ? *engine : Engine{};
    SetWindowTextW(panel.name, panel.draft.name.c_str());
    SetWindowTextW(panel.url, panel.draft.url.c_str());
    SetWindowTextW(panel.alias, panel.draft.alias.c_str());
    SetWindowTextW(panel.save, engine ? Tr(Text::saveChanges) : Tr(Text::add));
    ShowPage(enginePage, true);
    SetFocus(panel.name);
}

void CloseEngineEditor()
{
    ShowPage(enginePage);
    SetFocus(panel.addEngine);
}

std::wstring ReadControl(HWND control)
{
    std::wstring result(GetWindowTextLengthW(control) + 1, L'\0');
    result.resize(GetWindowTextW(control, result.data(), static_cast<int>(result.size())));
    return result;
}

void SaveEngineDraft()
{
    panel.draft.name = ReadControl(panel.name);
    panel.draft.url = ReadControl(panel.url);
    panel.draft.alias = ReadControl(panel.alias);
    const bool adding = panel.draft.id.empty();
    switch (adding ? panel.application->addEngine(panel.draft) : panel.application->editEngine(panel.draft)) {
    case AddEngineResult::added:
        CloseEngineEditor();
        RefreshEngineList(adding ? panel.application->engines().back().id : panel.draft.id);
        if (panel.actions.enginesChanged) panel.actions.enginesChanged();
        SetFocus(panel.engines);
        SetWindowTextW(panel.status, adding ? Tr(Text::engineAdded) : Tr(Text::changesSaved));
        return;
    case AddEngineResult::invalidName:
        SetWindowTextW(panel.status, Tr(Text::enterEngineName));
        SetFocus(panel.name);
        return;
    case AddEngineResult::invalidUrl:
        SetWindowTextW(panel.status, Tr(Text::enterSearchUrl));
        SetFocus(panel.url);
        return;
    case AddEngineResult::invalidIcon:
        SetWindowTextW(panel.status, Tr(Text::invalidIcon));
        return;
    case AddEngineResult::invalidAlias:
        SetWindowTextW(panel.status, Tr(Text::invalidAlias));
        SetFocus(panel.alias);
        return;
    case AddEngineResult::duplicateAlias:
        SetWindowTextW(panel.status, Tr(Text::duplicateAlias));
        SetFocus(panel.alias);
        return;
    case AddEngineResult::saveFailed:
        SetWindowTextW(panel.status, Tr(Text::cannotSaveEngine));
        return;
    }
}

void ManageEngine(UINT command)
{
    const auto* selected = SelectedEngine();
    if (!selected) return;
    if (command == pinEngine) {
        if (panel.application->togglePinnedEngine(selected->id)) {
            UpdateEngineControls();
            InvalidateRect(panel.engines, nullptr, FALSE);
        } else SetWindowTextW(panel.status, Tr(Text::cannotSavePins));
        return;
    }
    if (command == defaultEngine) {
        if (panel.actions.changeEngine(selected->id)) {
            UpdateEngineControls();
            SetWindowTextW(panel.status, Tr(Text::defaultEngineSet));
        } else SetWindowTextW(panel.status, Tr(Text::cannotSaveDefaultEngine));
        return;
    }
    if (selected->group != EngineGroup::custom) return;
    if (command == editEngine) {
        StartEngineEditor(selected);
        return;
    }
    const auto id = selected->id;
    if (command == deleteEngine && MessageBoxW(panel.window, Tr(Text::confirmDeleteEngine),
        Tr(Text::appName), MB_YESNO | MB_DEFBUTTON2) != IDYES) return;
    const bool saved = command == deleteEngine ? panel.application->deleteEngine(id)
        : panel.application->moveEngine(id, command == moveEngineUp ? -1 : 1);
    if (!saved) {
        SetWindowTextW(panel.status, Tr(Text::cannotSaveEngineChanges));
        return;
    }
    RefreshEngineList(command == deleteEngine ? std::wstring_view{} : std::wstring_view(id));
    if (panel.actions.enginesChanged) panel.actions.enginesChanged();
    SetFocus(panel.engines);
    SetWindowTextW(panel.status, Tr(Text::enginesUpdated));
}

void UseHistoryRow(int index, bool remove)
{
    if (index < 0 || static_cast<size_t>(index) >= panel.historyRows.size()) return;
    const auto record = panel.historyRows[index];
    if (remove) {
        if (platform::DeleteHistory(record.id)) {
            ListBox_DeleteString(panel.history, index);
            panel.historyRows.erase(panel.historyRows.begin() + index);
            if (panel.historyRows.empty()) RefreshHistory();
            else ListBox_SetCurSel(panel.history, std::min(index, static_cast<int>(panel.historyRows.size()) - 1));
        } else SetWindowTextW(panel.status, Tr(Text::cannotDeleteRecord));
        return;
    }
    const auto* engine = panel.application->findEngine(record.engineId);
    if (!engine) {
        SetWindowTextW(panel.status, Tr(Text::historyEngineDeleted));
        return;
    }
    if (panel.application->search(record.query, engine, SearchOrigin::history) == SearchResult::opened) RefreshHistory();
    else SetWindowTextW(panel.status, Tr(Text::cannotOpenBrowser));
}

void PickIconColor()
{
    static std::array<COLORREF, 16> customColors{};
    const auto color = panel.draft.color;
    CHOOSECOLORW dialog{sizeof(dialog)};
    dialog.hwndOwner = panel.window;
    dialog.rgbResult = RGB((color >> 16) & 255, (color >> 8) & 255, color & 255);
    dialog.lpCustColors = customColors.data();
    dialog.Flags = CC_RGBINIT | CC_FULLOPEN;
    if (!ChooseColorW(&dialog)) return;
    panel.draft.color = (static_cast<std::uint32_t>(GetRValue(dialog.rgbResult)) << 16)
        | (static_cast<std::uint32_t>(GetGValue(dialog.rgbResult)) << 8) | GetBValue(dialog.rgbResult);
    panel.draft.icon.clear();
    panel.draft.svgIcon.clear();
    SetWindowTextW(panel.status, L"");
    InvalidateRect(panel.window, nullptr, FALSE);
}

bool ApplyIconFile(const wchar_t* path)
{
    auto svg = ReadSvgIconFile(path);
    if (!svg.empty()) {
        panel.draft.svgIcon = std::move(svg);
        panel.draft.icon.clear();
    } else {
        auto pixels = ReadIconFile(path);
        if (pixels.empty()) return false;
        panel.draft.icon = std::move(pixels);
        panel.draft.svgIcon.clear();
    }
    SetWindowTextW(panel.status, L"");
    InvalidateRect(panel.window, nullptr, FALSE);
    return true;
}

void PickIconFile()
{
    std::wstring path(32768, L'\0');
    OPENFILENAMEW dialog{sizeof(dialog)};
    dialog.hwndOwner = panel.window;
    dialog.lpstrFile = path.data();
    dialog.nMaxFile = static_cast<DWORD>(path.size());
    dialog.lpstrFilter = Tr(Text::iconFilter);
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    if (GetOpenFileNameW(&dialog) && !ApplyIconFile(path.c_str())) {
        SetWindowTextW(panel.status, Tr(Text::cannotReadIcon));
    }
}

void ChangeFont(const FontChoice& choice)
{
    LOGFONTW applied{};
    if (!panel.actions.changeFont(choice, applied)) {
        SetWindowTextW(panel.status, Tr(Text::cannotApplyFont));
        return;
    }
    panel.values.font = choice;
    panel.values.fontFace = applied;
    RefreshFontPreview();
    SetWindowTextW(panel.status, Tr(Text::fontApplied));
}

void PickSystemFont()
{
    LOGFONTW choice = panel.values.fontFace;
    choice.lfHeight = -Px(16);
    CHOOSEFONTW dialog{sizeof(dialog)};
    dialog.hwndOwner = panel.window;
    dialog.lpLogFont = &choice;
    dialog.Flags = CF_SCREENFONTS | CF_FORCEFONTEXIST | CF_NOVERTFONTS | CF_NOSIZESEL | CF_NOSTYLESEL;
    if (panel.values.font.source != FontSource::file) dialog.Flags |= CF_INITTOLOGFONTSTRUCT;
    if (ChooseFontW(&dialog)) ChangeFont({FontSource::system, choice.lfFaceName});
}

void PickFontFile()
{
    std::wstring path(32768, L'\0');
    OPENFILENAMEW dialog{sizeof(dialog)};
    dialog.hwndOwner = panel.window;
    dialog.lpstrFilter = Tr(Text::fontFilter);
    dialog.lpstrFile = path.data();
    dialog.nMaxFile = static_cast<DWORD>(path.size());
    dialog.lpstrTitle = Tr(Text::chooseFontFile);
    dialog.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    if (GetOpenFileNameW(&dialog)) {
        path.resize(std::char_traits<wchar_t>::length(path.c_str()));
        ChangeFont({FontSource::file, path});
    }
}

void PaintSettings(HWND window, HDC dc, const RECT& invalid)
{
    YISOU_PERF_SCOPE("paint_settings");
    RECT bounds{};
    GetClientRect(window, &bounds);
    if (panel.surfaceBrush) FillRect(dc, &invalid, panel.surfaceBrush);
    const auto [left, width] = ContentArea(MulDiv(bounds.right, USER_DEFAULT_SCREEN_DPI, panel.dpi));
    const float scroll = static_cast<float>(panel.scroll) * USER_DEFAULT_SCREEN_DPI / panel.dpi;
    const bool appearance = panel.page == appearancePage;
    const std::array tops{firstCardTop, secondCardTop, previewCardTop, engineMenuTop};
    const std::array heights{72, 72, 116, 66};
    const std::array iconOffsets{25, 25, 18, 22};
    const std::array appearanceIcons{TablerIcon::typography, TablerIcon::fileTypography, TablerIcon::eye, TablerIcon::search};

    // Bind stripes of the invalid content so Direct2D doesn't allocate a bitmap the size of the window.
    RECT content{Px(left), std::max(0L, invalid.top), Px(left + width), std::min(bounds.bottom, invalid.bottom)};
    IntersectRect(&content, &content, &invalid);
    const float dipPerPixel = static_cast<float>(USER_DEFAULT_SCREEN_DPI) / panel.dpi;
    constexpr LONG stripeHeight = 96;
    for (LONG stripeTop = content.top; stripeTop < content.bottom; stripeTop += stripeHeight) {
        const RECT stripe{content.left, stripeTop, content.right, std::min(content.bottom, stripeTop + stripeHeight)};
        if (!BeginShapes(dc, stripe)) break;
        panel.drawing->SetTransform(D2D1::Matrix3x2F::Translation(-stripe.left * dipPerPixel, -stripe.top * dipPerPixel));
        panel.drawing->Clear(Color(panel.colors.surface));
        if (panel.page == configurationPage) {
            for (const auto& card : {D2D1::RectF(left + .5f, 116.f - scroll, left + width - .5f, 247.f - scroll),
                D2D1::RectF(left + .5f, 260.f - scroll, left + width - .5f, 360.f - scroll)}) {
                RoundedFill(card, cardRadius, panel.colors.card);
                RoundedStroke(card, cardRadius, panel.colors.border);
            }
        }
        if (panel.page == generalPage) {
            for (size_t i = 0; i <= generalOptions.size(); ++i) {
                const float y = generalCardTop + static_cast<float>(i) * generalCardStep - scroll;
                const auto card = D2D1::RectF(left + .5f, y + .5f, left + width - .5f, y + 66.f);
                RoundedFill(card, cardRadius, panel.colors.card);
                RoundedStroke(card, cardRadius, panel.colors.border);
            }
        }
        for (size_t i = 0; appearance && i < tops.size(); ++i) {
            const float y = static_cast<float>(tops[i]) - scroll;
            const auto card = D2D1::RectF(left + 0.5f, y + 0.5f, left + width - 0.5f, y + heights[i] - 0.5f);
            RoundedFill(card, cardRadius, panel.colors.card);
            RoundedStroke(card, cardRadius, panel.colors.border);
        }
        if (panel.page == enginePage) {
            const float bottom = static_cast<float>(bounds.bottom) * USER_DEFAULT_SCREEN_DPI / panel.dpi;
            const auto card = panel.editing ? D2D1::RectF(left + 0.5f, editorCardTop - scroll,
                left + width - 0.5f, 361 - scroll) : D2D1::RectF(left + 0.5f, 123, left + width - 0.5f, bottom - 78);
            RoundedFill(card, cardRadius, panel.colors.card);
            RoundedStroke(card, cardRadius, panel.colors.border);
        }
        if (panel.page == shortcutPage) {
            for (const auto& card : {D2D1::RectF(left + .5f, 116.f - scroll, left + width - .5f, 233.f - scroll),
                D2D1::RectF(left + .5f, 248.f - scroll, left + width - .5f, 409.f - scroll),
                D2D1::RectF(left + .5f, 423.f - scroll, left + width - .5f, 530.f - scroll),
                D2D1::RectF(left + .5f, 543.f - scroll, left + width - .5f, 629.f - scroll)}) {
                RoundedFill(card, cardRadius, panel.colors.card);
                RoundedStroke(card, cardRadius, panel.colors.border);
            }
            const auto field = D2D1::RectF(left + 18.f, 583.f - scroll, left + width - 123.f, 617.f - scroll);
            RoundedFill(field, controlRadius, panel.colors.button);
            RoundedStroke(field, controlRadius, GetFocus() == panel.excludedApps ? panel.colors.accent : panel.colors.border);
        }
        if (panel.page == historyPage) {
            const float split = static_cast<float>(left + width * 3 / 5);
            for (const auto& [field, range] : {std::pair{panel.historyKeyword, std::pair{static_cast<float>(left), split - 4}},
                {panel.historySource, {split + 5, static_cast<float>(left + width)}}}) {
                const auto frame = D2D1::RectF(range.first, historyFiltersTop, range.second, historyFiltersTop + 34.f);
                RoundedFill(frame, controlRadius, panel.colors.button);
                RoundedStroke(frame, controlRadius, GetFocus() == field ? panel.colors.accent : panel.colors.border);
            }
        }
        if (panel.page == browserPage && panel.browserPanel != BrowserPanel::closed) {
            const float bottom = panel.browserPanel == BrowserPanel::path ? 235.f
                : std::min(static_cast<float>(bounds.bottom) * USER_DEFAULT_SCREEN_DPI / panel.dpi - 77.f, 390.f);
            const auto card = D2D1::RectF(left + .5f, 181.f, left + width - .5f, bottom + 1.f);
            RoundedFill(card, cardRadius, panel.colors.card);
            RoundedStroke(card, cardRadius, panel.colors.border);
            if (panel.browserPanel == BrowserPanel::path) {
                const auto field = D2D1::RectF(left + 8.f, 193.f, left + width - 105.f, 227.f);
                RoundedFill(field, controlRadius, panel.colors.button);
                RoundedStroke(field, controlRadius, GetFocus() == panel.browserPath ? panel.colors.accent : panel.colors.border);
            }
        }
        if (panel.editing) {
            const int nameEnd = left + 20 + (width - 40) * 3 / 5;
            for (HWND control : {panel.name, panel.alias, panel.url}) {
                const int top = control == panel.url ? urlFieldTop : nameFieldTop;
                const int fieldLeft = control == panel.alias ? nameEnd + 12 : left + 20;
                const int fieldRight = control == panel.name ? nameEnd : left + width - 20;
                const auto field = D2D1::RectF(static_cast<float>(fieldLeft), top - scroll, static_cast<float>(fieldRight), top + 35 - scroll);
                RoundedFill(field, controlRadius, panel.colors.button);
                const bool focused = GetFocus() == control;
                RoundedStroke(field, controlRadius, focused ? panel.colors.accent : panel.colors.border, focused ? 1.5f : 1.f);
            }
        }
        EndShapes();
    }

    for (size_t i = 0; appearance && i < tops.size(); ++i) {
        const int iconTop = Px(tops[i] + iconOffsets[i]) - panel.scroll;
        const RECT icon{Px(left + 16), iconTop, Px(left + 37), iconTop + Px(22)};
        DrawTablerIcon(dc, appearanceIcons[i], icon, panel.colors.secondary);
    }
    if (panel.editing) {
        const int iconX = Px(left + 20);
        const int iconY = Px(307) - panel.scroll;
        DrawEngineIcon(dc, panel.draft, {iconX, iconY, iconX + Px(42), iconY + Px(42)});
    }
}

void PaintNavigation(const DRAWITEMSTRUCT& item)
{
    const auto& bounds = item.rcItem;
    if (panel.surfaceBrush) FillRect(item.hDC, &bounds, panel.surfaceBrush);
    if (item.itemID >= pageNames.size()) return;

    const bool selected = static_cast<int>(item.itemID) == ListBox_GetCurSel(panel.nav);
    const bool hovered = static_cast<int>(item.itemID) == panel.hoveredPage;
    const float width = static_cast<float>(bounds.right - bounds.left) * USER_DEFAULT_SCREEN_DPI / panel.dpi;
    if (BeginShapes(item.hDC, bounds)) {
        panel.drawing->Clear(Color(panel.colors.surface));
        if (selected || hovered) {
            RoundedFill(D2D1::RectF(0, 2, width - 1, navRowHeight - 2.f), 5,
                selected ? panel.colors.selection : panel.colors.hover);
        }
        if (selected) {
            RoundedFill(D2D1::RectF(0, 13, 3, navRowHeight - 13.f), 1.5f, panel.colors.accent);
        }
        if ((item.itemState & (ODS_FOCUS | ODS_NOFOCUSRECT)) == ODS_FOCUS) {
            RoundedStroke(D2D1::RectF(2, 3, width - 3, navRowHeight - 3.f), 4, panel.colors.accent);
        }
        EndShapes();
    }
    const RECT icon{bounds.left + Px(14), bounds.top, bounds.left + Px(35), bounds.bottom};
    const RECT label{bounds.left + Px(47), bounds.top, bounds.right - Px(8), bounds.bottom};
    DrawTablerIcon(item.hDC, pageIcons[item.itemID], icon, panel.colors.accent);
    DrawTextWithFont(item.hDC, Tr(pageNames[item.itemID]), label,
        panel.bodyFont, panel.colors.text, DT_LEFT);
}

void PaintEngineRow(const DRAWITEMSTRUCT& item)
{
    const auto& engines = panel.application->engines();
    const auto& bounds = item.rcItem;
    if (panel.cardBrush) FillRect(item.hDC, &bounds, panel.cardBrush);
    if (item.itemID >= engines.size()) return;
    const auto& engine = engines[item.itemID];
    const bool selected = engine.id == panel.application->preferences().engine;
    const bool focused = (item.itemState & ODS_SELECTED) != 0 || (item.itemState & (ODS_FOCUS | ODS_NOFOCUSRECT)) == ODS_FOCUS;
    const float width = static_cast<float>(bounds.right - bounds.left) * USER_DEFAULT_SCREEN_DPI / panel.dpi;
    if (BeginShapes(item.hDC, bounds)) {
        panel.drawing->Clear(Color(panel.colors.card));
        const auto row = D2D1::RectF(1, 1, width - 2, engineRowHeight - 2.f);
        if (selected) RoundedFill(row, 6, panel.colors.selection);
        if (focused) RoundedStroke(row, 6, panel.colors.accent);
        if (!selected && !focused) {
            panel.brush->SetColor(Color(panel.colors.border));
            panel.drawing->DrawLine(D2D1::Point2F(48, engineRowHeight - 1.f),
                D2D1::Point2F(width - 12, engineRowHeight - 1.f), panel.brush.Get());
        }
        EndShapes();
    }
    const LONG x = bounds.left + Px(13);
    const LONG y = bounds.top + Px(14);
    DrawEngineIcon(item.hDC, engine, {x, y, x + Px(24), y + Px(24)});
    const RECT name{bounds.left + Px(50), bounds.top + Px(5), bounds.right - Px(43), bounds.top + Px(27)};
    const RECT group{bounds.left + Px(50), bounds.top + Px(26), bounds.right - Px(43), bounds.top + Px(46)};
    DrawTextWithFont(item.hDC, i18n::EngineName(engine.id, engine.name.c_str()), name,
        panel.bodyFont, panel.colors.text, DT_END_ELLIPSIS);
    std::wstring detail = GroupName(engine.group);
    if (!engine.alias.empty()) detail += L"  ·  #" + engine.alias;
    if (panel.application->isPinned(engine.id)) detail += Tr(Text::pinnedSuffix);
    DrawTextWithFont(item.hDC, detail.c_str(), group, panel.detailFont, panel.colors.secondary, DT_END_ELLIPSIS);
    if (selected) {
        const RECT tick{bounds.right - Px(32), bounds.top, bounds.right - Px(14), bounds.bottom};
        DrawTablerIcon(item.hDC, TablerIcon::check, tick, panel.colors.accent);
    }
}

RECT HistoryActionRect(const RECT& row, bool remove)
{
    const LONG right = row.right - Px(remove ? 15 : 81);
    return {right - Px(remove ? 54 : 84), row.bottom - Px(33), right, row.bottom - Px(8)};
}

void PaintHistoryRow(const DRAWITEMSTRUCT& item)
{
    const auto& bounds = item.rcItem;
    if (panel.surfaceBrush) FillRect(item.hDC, &bounds, panel.surfaceBrush);
    if (item.itemID >= panel.historyRows.size()) return;
    const auto& row = panel.historyRows[item.itemID];
    const bool focused = (item.itemState & (ODS_FOCUS | ODS_NOFOCUSRECT)) == ODS_FOCUS;
    if (BeginShapes(item.hDC, bounds)) {
        panel.drawing->Clear(Color(panel.colors.surface));
        const float width = static_cast<float>(bounds.right - bounds.left) * USER_DEFAULT_SCREEN_DPI / panel.dpi;
        const float height = static_cast<float>(bounds.bottom - bounds.top) * USER_DEFAULT_SCREEN_DPI / panel.dpi;
        const auto card = D2D1::RectF(1, 1, width - 2, height - 6.f);
        RoundedFill(card, cardRadius, panel.colors.card);
        RoundedStroke(card, cardRadius, focused ? panel.colors.accent : panel.colors.border);
        EndShapes();
    }
    const auto label = [&](const std::wstring& value, int y, int rowHeight, HFONT font, COLORREF color, int right = 15) {
        DrawTextWithFont(item.hDC, value.c_str(),
            {bounds.left + Px(16), bounds.top + Px(y), bounds.right - Px(right), bounds.top + Px(y + rowHeight)},
            font, color, DT_END_ELLIPSIS);
    };
    auto query = row.query;
    std::ranges::replace(query, L'\r', L' ');
    std::ranges::replace(query, L'\n', L' ');
    label(query, 11, 28, panel.bodyFont, panel.colors.text);
    const auto time = platform::HistoryTime(row.id);
    const auto* engineName = i18n::EngineName(row.engineId, row.engineName.c_str());
    label(engineName + (L"  ·  " + row.browser + L"  ·  " + time), 40, 25, panel.detailFont, panel.colors.secondary);
    if (row.origin == SearchOrigin::selection) {
        const int y = MulDiv(bounds.bottom - bounds.top, USER_DEFAULT_SCREEN_DPI, panel.dpi) - 34;
        label(row.source.empty() ? Tr(Text::selection) : i18n::Format(Text::selectionSource, {row.source}),
            y, 26, panel.detailFont, panel.colors.secondary, 175);
    }
    DrawTextWithFont(item.hDC, Tr(Text::searchAgain), HistoryActionRect(bounds, false), panel.detailFont, panel.colors.accent, DT_CENTER);
    DrawTextWithFont(item.hDC, Tr(Text::deleteItem), HistoryActionRect(bounds, true), panel.detailFont, panel.colors.secondary, DT_CENTER);
}

void PaintBrowserLabels(HDC dc, const BrowserRow& row, RECT bounds)
{
    const LONG iconWidth = Px(23);
    const LONG x = bounds.left + Px(13);
    const LONG y = (bounds.top + bounds.bottom - iconWidth) / 2;
    if (row.icon) DrawIconEx(dc, x, y, row.icon.get(), iconWidth, iconWidth, 0, nullptr, DI_NORMAL);
    else DrawTablerIcon(dc, TablerIcon::browser, {x, y, x + iconWidth, y + iconWidth}, panel.colors.secondary);
    const LONG middle = (bounds.top + bounds.bottom) / 2;
    const LONG right = bounds.right - Px(40);
    if (row.detail.empty()) {
        DrawTextWithFont(dc, row.name.c_str(), {x + Px(36), bounds.top, right, bounds.bottom}, panel.bodyFont,
            panel.colors.text, DT_END_ELLIPSIS);
        return;
    }
    DrawTextWithFont(dc, row.name.c_str(), {x + Px(36), middle - Px(19), right, middle + Px(2)}, panel.bodyFont,
        panel.colors.text, DT_END_ELLIPSIS);
    DrawTextWithFont(dc, row.detail.c_str(), {x + Px(36), middle + Px(2), right, middle + Px(18)}, panel.detailFont,
        panel.colors.secondary, DT_PATH_ELLIPSIS);
}

const BrowserRow* CurrentBrowserRow()
{
    const auto& selected = panel.application->preferences().browser;
    const auto row = std::ranges::find_if(panel.browserRows, [&](const BrowserRow& item) { return SameBrowser(item.path, selected); });
    return row == panel.browserRows.end() ? nullptr : &*row;
}

void PaintBrowserRow(const DRAWITEMSTRUCT& item)
{
    const auto& bounds = item.rcItem;
    if (panel.cardBrush) FillRect(item.hDC, &bounds, panel.cardBrush);
    if (item.itemID >= panel.browserRows.size()) return;
    const auto& row = panel.browserRows[item.itemID];
    const bool selected = SameBrowser(row.path, panel.application->preferences().browser);
    const bool focused = (item.itemState & (ODS_FOCUS | ODS_NOFOCUSRECT)) == ODS_FOCUS;
    if (BeginShapes(item.hDC, bounds)) {
        panel.drawing->Clear(Color(panel.colors.card));
        const auto face = D2D1::RectF(1, 1, static_cast<float>(bounds.right - bounds.left) * 96 / panel.dpi - 2,
            browserRowHeight - 2.f);
        if (selected || focused) RoundedFill(face, 6, selected ? panel.colors.selection : panel.colors.hover);
        if (focused) RoundedStroke(face, 6, panel.colors.accent);
        EndShapes();
    }
    PaintBrowserLabels(item.hDC, row, bounds);
    if (selected) DrawTablerIcon(item.hDC, TablerIcon::check,
        {bounds.right - Px(31), bounds.top, bounds.right - Px(13), bounds.bottom}, panel.colors.accent);
}

void PaintShortcutCaps(const DRAWITEMSTRUCT& item)
{
    const bool selection = item.hwndItem == panel.selectionRecord;
    const bool clipboard = item.hwndItem == panel.clipboardRecord;
    const bool recording = panel.recording == item.hwndItem;
    const auto& preferences = panel.application->preferences();
    const auto shortcut = recording ? panel.shortcutDraft : clipboard ? preferences.clipboardShortcut : !selection ? std::optional{preferences.shortcut}
        : preferences.selectionShortcut ? std::optional{preferences.selectionShortcut->shortcut} : std::nullopt;
    if (!shortcut) {
        DrawTextWithFont(item.hDC, recording ? (selection || clipboard ? Tr(Text::pressKeyCombination) : Tr(Text::pressShortcut))
            : clipboard ? Tr(Text::recordClipboardShortcut) : Tr(Text::recordSelectionShortcut),
            item.rcItem, panel.bodyFont, panel.colors.text, DT_CENTER);
        return;
    }
    const auto caps = ShortcutCaps(*shortcut);
    RECT area = item.rcItem;
    if (!recording) {
        area.right -= Px(49);
        DrawTextWithFont(item.hDC, Tr(Text::change), {area.right, area.top, item.rcItem.right - Px(5), area.bottom},
            panel.bodyFont, panel.colors.accent, DT_CENTER);
    }
    const int available = area.right - area.left - Px(12);
    const int stride = std::min(Px(63), available / static_cast<int>(caps.size()));
    int left = area.left + (area.right - area.left - stride * static_cast<int>(caps.size())) / 2;
    for (const auto& cap : caps) {
        const RECT key{left + Px(2), item.rcItem.top + Px(5), left + stride - Px(2), item.rcItem.bottom - Px(5)};
        if (BeginShapes(item.hDC, key)) {
            const auto face = D2D1::RectF(.5f, .5f, static_cast<float>(key.right - key.left) * 96 / panel.dpi - .5f, 23.f);
            RoundedFill(face, 4, panel.colors.card);
            RoundedStroke(face, 4, panel.colors.border);
            EndShapes();
        }
        DrawTextWithFont(item.hDC, cap.c_str(), key, panel.detailFont, panel.colors.text, DT_CENTER | DT_END_ELLIPSIS);
        left += stride;
    }
}

void PaintButton(const DRAWITEMSTRUCT& item)
{
    const auto& bounds = item.rcItem;
    if (panel.cardBrush) FillRect(item.hDC, &bounds, panel.cardBrush);
    const bool disabled = (item.itemState & ODS_DISABLED) != 0;
    const bool pressed = (item.itemState & ODS_SELECTED) != 0;
    const bool hovered = panel.hoveredButton == item.hwndItem;
    const bool focused = (item.itemState & (ODS_FOCUS | ODS_NOFOCUSRECT)) == ODS_FOCUS;
    const bool general = item.CtlID >= startWithWindows && item.CtlID <= closeToTray;
    const bool generalEnabled = general && GeneralOptionEnabled(static_cast<ControlId>(item.CtlID));
    const bool showLabels = panel.application->preferences().showEngineTileNames;
    const auto currentMode = showLabels ? engineMenuWithLabels : engineMenuIconsOnly;
    const bool selectedMode = item.CtlID == static_cast<UINT>(currentMode);
    const bool primary = !disabled && (item.hwndItem == panel.save || item.hwndItem == panel.addEngine
        || item.hwndItem == panel.shortcutSave || item.hwndItem == panel.browserSave
        || (item.hwndItem == panel.historyToggle && panel.application->preferences().recordHistory) || generalEnabled || selectedMode);
    const bool onSurface = item.hwndItem == panel.save || item.hwndItem == panel.cancel || item.hwndItem == panel.addEngine
        || item.hwndItem == panel.browserSelect || item.hwndItem == panel.browserOther
        || item.hwndItem == panel.browserSave || item.hwndItem == panel.browserCancel
        || item.hwndItem == panel.historyToggle || item.hwndItem == panel.historyMore
        || item.hwndItem == panel.engineDefault || item.hwndItem == panel.enginePin || item.hwndItem == panel.engineEdit || item.hwndItem == panel.engineDelete
        || item.hwndItem == panel.engineUp || item.hwndItem == panel.engineDown || item.hwndItem == panel.engineTest;
    const auto background = onSurface ? panel.colors.surface : panel.colors.card;
    if (onSurface && panel.surfaceBrush) FillRect(item.hDC, &bounds, panel.surfaceBrush);
    const float scale = static_cast<float>(USER_DEFAULT_SCREEN_DPI) / panel.dpi;
    const auto face = D2D1::RectF(0.5f, 0.5f,
        (bounds.right - bounds.left) * scale - 0.5f, (bounds.bottom - bounds.top) * scale - 0.5f);
    if (BeginShapes(item.hDC, bounds)) {
        panel.drawing->Clear(Color(background));
        const COLORREF fill = disabled ? panel.colors.card
            : primary ? panel.colors.primary : pressed ? panel.colors.pressed : hovered ? panel.colors.hover : panel.colors.button;
        RoundedFill(face, controlRadius, fill);
        RoundedStroke(face, controlRadius, focused ? panel.colors.text : primary ? panel.colors.primary : panel.colors.border, focused ? 1.5f : 1.f);
        if (primary && (pressed || hovered)) {
            auto shade = D2D1::ColorF(0x000000, pressed ? 0.18f : 0.10f);
            panel.brush->SetColor(shade);
            panel.drawing->FillRoundedRectangle(D2D1::RoundedRect(face, controlRadius, controlRadius), panel.brush.Get());
        }
        EndShapes();
    }
    if (item.hwndItem == panel.shortcutRecord || item.hwndItem == panel.selectionRecord || item.hwndItem == panel.clipboardRecord) {
        PaintShortcutCaps(item);
        return;
    }
    if (item.hwndItem == panel.browserSelect) {
        if (const auto* row = CurrentBrowserRow()) PaintBrowserLabels(item.hDC, *row, bounds);
        DrawTablerIcon(item.hDC, panel.browserPanel == BrowserPanel::closed ? TablerIcon::chevronDown : TablerIcon::chevronUp,
            {bounds.right - Px(33), bounds.top, bounds.right - Px(15), bounds.bottom}, panel.colors.secondary);
        return;
    }
    wchar_t label[64]{};
    if (!general) GetWindowTextW(item.hwndItem, label, static_cast<int>(std::size(label)));
    DrawTextWithFont(item.hDC, general ? (generalEnabled ? Tr(Text::on) : Tr(Text::off)) : label, bounds, panel.bodyFont,
        disabled ? panel.colors.secondary : primary ? panel.colors.primaryText : panel.colors.text, DT_CENTER);
}

LRESULT CALLBACK SettingsControlProc(HWND control, UINT message, WPARAM key, LPARAM data, UINT_PTR, DWORD_PTR)
{
    const bool navigationControl = control == panel.nav;
    switch (message) {
    case WM_LBUTTONUP:
        if (control == panel.history) {
            const POINT point{GET_X_LPARAM(data), GET_Y_LPARAM(data)};
            const auto hit = SendMessageW(control, LB_ITEMFROMPOINT, 0, data);
            const int index = LOWORD(hit);
            RECT row{};
            const auto result = DefSubclassProc(control, message, key, data);
            if (HIWORD(hit) || ListBox_GetItemRect(control, index, &row) == LB_ERR) return result;
            const auto remove = HistoryActionRect(row, true);
            const auto replay = HistoryActionRect(row, false);
            if (PtInRect(&remove, point)) UseHistoryRow(index, true);
            else if (PtInRect(&replay, point)) UseHistoryRow(index, false);
            return result;
        }
        if (control == panel.browsers) {
            const POINT point{GET_X_LPARAM(data), GET_Y_LPARAM(data)};
            const auto hit = SendMessageW(control, LB_ITEMFROMPOINT, 0, data);
            RECT bounds{};
            const int row = LOWORD(hit);
            const auto result = DefSubclassProc(control, message, key, data);
            if (!HIWORD(hit) && ListBox_GetItemRect(control, row, &bounds) != LB_ERR
                && PtInRect(&bounds, point) && row == ListBox_GetCurSel(control)) ApplyBrowserRow(row);
            return result;
        }
        break;
    case WM_SETFOCUS:
    case WM_KILLFOCUS:
        if (control == panel.name || control == panel.url || control == panel.alias
            || control == panel.historyKeyword || control == panel.historySource || control == panel.browserPath) {
            InvalidateRect(panel.window, nullptr, FALSE);
        }
        break;
    case WM_MOUSEMOVE: {
        if (control == panel.name || control == panel.url || control == panel.alias || control == panel.history
            || control == panel.historyKeyword || control == panel.historySource || control == panel.browserPath) break;
        TRACKMOUSEEVENT tracking{sizeof(tracking), TME_LEAVE, control, 0};
        if (navigationControl) {
            POINT point{GET_X_LPARAM(data), GET_Y_LPARAM(data)};
            ClientToScreen(control, &point);
            const int hovered = LBItemFromPt(control, point, FALSE);
            if (panel.hoveredPage != hovered) {
                panel.hoveredPage = hovered;
                TrackMouseEvent(&tracking);
                InvalidateRect(control, nullptr, FALSE);
            }
        } else if (panel.hoveredButton != control) {
            panel.hoveredButton = control;
            TrackMouseEvent(&tracking);
            InvalidateRect(control, nullptr, FALSE);
        }
        break;
    }
    case WM_MOUSELEAVE:
        if (navigationControl) panel.hoveredPage = -1;
        else if (panel.hoveredButton == control) panel.hoveredButton = nullptr;
        InvalidateRect(control, nullptr, FALSE);
        break;
    }
    return DefSubclassProc(control, message, key, data);
}

LRESULT CALLBACK SettingsProc(HWND window, UINT message, WPARAM key, LPARAM data)
{
    switch (message) {
    case WM_NCHITTEST: {
        const LRESULT hit = DefWindowProcW(window, message, key, data);
        if (hit != HTCLIENT) return hit;

        POINT point{GET_X_LPARAM(data), GET_Y_LPARAM(data)};
        ScreenToClient(window, &point);
        const HWND child = ChildWindowFromPointEx(window, point, CWP_SKIPINVISIBLE);
        if (child && child != window && (GetWindowLongPtrW(child, GWL_STYLE) & WS_TABSTOP)) return HTCLIENT;
        return HTCAPTION;
    }
    case WM_CREATE:
        panel.window = window;
        panel.dpi = GetDpiForWindow(window);
        RefreshPalette();
        panel.nav = Control(L"LISTBOX", L"", LBS_NOTIFY | LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED
            | LBS_HASSTRINGS | WS_TABSTOP | WS_VSCROLL, navigation);
        panel.title = Control(L"STATIC", L"", WS_VISIBLE);
        panel.section = Control(L"STATIC", L"", WS_VISIBLE);
        panel.fontHeading = Control(L"STATIC", Tr(Text::searchBoxFont));
        panel.fontName = Control(L"STATIC", L"", SS_ENDELLIPSIS);
        panel.fileHeading = Control(L"STATIC", Tr(Text::localFontFile));
        panel.fontPath = Control(L"STATIC", L"", SS_PATHELLIPSIS);
        panel.previewHeading = Control(L"STATIC", Tr(Text::preview));
        panel.preview = Control(L"STATIC", Tr(Text::fontPreview), SS_ENDELLIPSIS | SS_CENTERIMAGE);
        panel.systemButton = Control(L"BUTTON", Tr(Text::chooseSystemFont), BS_OWNERDRAW | WS_TABSTOP, systemFont);
        panel.fileButton = Control(L"BUTTON", Tr(Text::openFile), BS_OWNERDRAW | WS_TABSTOP, localFont);
        panel.resetButton = Control(L"BUTTON", Tr(Text::resetDefault), BS_OWNERDRAW | WS_TABSTOP, resetFont);
        panel.engineMenuTitle = Control(L"STATIC", Tr(Text::engineMenuLayout));
        panel.engineMenuLabels = Control(L"BUTTON", Tr(Text::engineMenuWithLabels), BS_OWNERDRAW | WS_TABSTOP, engineMenuWithLabels);
        panel.engineMenuIcons = Control(L"BUTTON", Tr(Text::engineMenuIconsOnly), BS_OWNERDRAW | WS_TABSTOP, engineMenuIconsOnly);
        panel.engines = Control(L"LISTBOX", L"", LBS_NOTIFY | LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED
            | LBS_HASSTRINGS | WS_TABSTOP | WS_VSCROLL, engineList);
        panel.addEngine = Control(L"BUTTON", Tr(Text::addSearchEngine), BS_OWNERDRAW | WS_TABSTOP, newEngine);
        panel.nameLabel = Control(L"STATIC", Tr(Text::name));
        panel.name = Control(L"EDIT", L"", ES_AUTOHSCROLL | WS_TABSTOP, engineName);
        panel.urlLabel = Control(L"STATIC", Tr(Text::searchUrl));
        panel.url = Control(L"EDIT", L"", ES_AUTOHSCROLL | WS_TABSTOP, engineUrl);
        panel.urlHint = Control(L"STATIC", Tr(Text::searchUrlHint));
        panel.iconLabel = Control(L"STATIC", Tr(Text::icon));
        panel.iconColor = Control(L"BUTTON", Tr(Text::chooseColor), BS_OWNERDRAW | WS_TABSTOP, chooseIconColor);
        panel.iconFile = Control(L"BUTTON", Tr(Text::importIcon), BS_OWNERDRAW | WS_TABSTOP, chooseIconFile);
        panel.cancel = Control(L"BUTTON", Tr(Text::cancel), BS_OWNERDRAW | WS_TABSTOP, cancelEngine);
        panel.save = Control(L"BUTTON", Tr(Text::add), BS_OWNERDRAW | WS_TABSTOP, saveEngine);
        panel.shortcutTitle = Control(L"STATIC", Tr(Text::openYiSou));
        panel.shortcutHint = Control(L"STATIC", L"", SS_ENDELLIPSIS);
        panel.shortcutRecord = Control(L"BUTTON", Tr(Text::changeShortcut), BS_OWNERDRAW | WS_TABSTOP, recordShortcut);
        panel.shortcutSave = Control(L"BUTTON", Tr(Text::save), BS_OWNERDRAW | WS_TABSTOP, saveShortcut);
        panel.shortcutCancel = Control(L"BUTTON", Tr(Text::cancel), BS_OWNERDRAW | WS_TABSTOP, cancelShortcut);
        panel.shortcutReset = Control(L"BUTTON", Tr(Text::resetDefault), BS_OWNERDRAW | WS_TABSTOP, resetShortcut);
        panel.selectionTitle = Control(L"STATIC", Tr(Text::selectionSearch));
        panel.selectionHint = Control(L"STATIC", L"", SS_ENDELLIPSIS);
        panel.selectionEngines = Control(L"COMBOBOX", L"", CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP, selectionEngineList);
        panel.selectionMode = Control(L"COMBOBOX", L"", CBS_DROPDOWNLIST | WS_TABSTOP, selectionMode);
        panel.selectionRecord = Control(L"BUTTON", Tr(Text::recordSelectionShortcut), BS_OWNERDRAW | WS_TABSTOP, recordSelectionShortcut);
        panel.selectionClear = Control(L"BUTTON", Tr(Text::clear), BS_OWNERDRAW | WS_TABSTOP, clearSelectionShortcut);
        panel.browserSelect = Control(L"BUTTON", Tr(Text::chooseBrowser), BS_OWNERDRAW | WS_TABSTOP, selectBrowser);
        panel.browsers = Control(L"LISTBOX", Tr(Text::availableBrowsers), LBS_NOTIFY | LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED
            | LBS_HASSTRINGS | WS_TABSTOP | WS_VSCROLL, browserList);
        panel.browserOther = Control(L"BUTTON", Tr(Text::chooseAnotherBrowser), BS_OWNERDRAW | WS_TABSTOP, otherBrowser);
        panel.browserPath = Control(L"EDIT", L"", ES_AUTOHSCROLL | WS_TABSTOP, browserLocation);
        panel.browserBrowse = Control(L"BUTTON", Tr(Text::browse), BS_OWNERDRAW | WS_TABSTOP, browseBrowser);
        panel.browserSave = Control(L"BUTTON", Tr(Text::save), BS_OWNERDRAW | WS_TABSTOP, saveBrowser);
        panel.browserCancel = Control(L"BUTTON", Tr(Text::cancel), BS_OWNERDRAW | WS_TABSTOP, cancelBrowser);
        panel.aliasLabel = Control(L"STATIC", Tr(Text::optionalAlias));
        panel.alias = Control(L"EDIT", L"", ES_AUTOHSCROLL | WS_TABSTOP, engineAlias);
        panel.engineEdit = Control(L"BUTTON", Tr(Text::edit), BS_OWNERDRAW | WS_TABSTOP, editEngine);
        panel.engineDelete = Control(L"BUTTON", Tr(Text::deleteItem), BS_OWNERDRAW | WS_TABSTOP, deleteEngine);
        panel.engineUp = Control(L"BUTTON", Tr(Text::moveUp), BS_OWNERDRAW | WS_TABSTOP, moveEngineUp);
        panel.engineDown = Control(L"BUTTON", Tr(Text::moveDown), BS_OWNERDRAW | WS_TABSTOP, moveEngineDown);
        panel.engineDefault = Control(L"BUTTON", Tr(Text::setDefault), BS_OWNERDRAW | WS_TABSTOP, defaultEngine);
        panel.enginePin = Control(L"BUTTON", Tr(Text::pin), BS_OWNERDRAW | WS_TABSTOP, pinEngine);
        panel.engineTest = Control(L"BUTTON", Tr(Text::testSearch), BS_OWNERDRAW | WS_TABSTOP, testEngine);
        panel.history = Control(L"LISTBOX", Tr(Text::searchHistory), LBS_NOTIFY | LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED
            | LBS_HASSTRINGS | WS_TABSTOP | WS_VSCROLL, historyList);
        panel.historyToggle = Control(L"BUTTON", Tr(Text::historyOff), BS_OWNERDRAW | WS_TABSTOP, toggleHistory);
        panel.historyMore = Control(L"BUTTON", Tr(Text::loadMore), BS_OWNERDRAW | WS_TABSTOP, moreHistory);
        panel.historyEmpty = Control(L"STATIC", Tr(Text::noHistory));
        panel.historyKeyword = Control(L"EDIT", L"", ES_AUTOHSCROLL | WS_TABSTOP, historyKeyword);
        panel.historySource = Control(L"EDIT", L"", ES_AUTOHSCROLL | WS_TABSTOP, historySource);
        for (size_t i = 0; i < generalOptions.size(); ++i) {
            const auto& [id, title, detail] = generalOptions[i];
            panel.generalTitles[i] = Control(L"STATIC", Tr(title), SS_ENDELLIPSIS);
            panel.generalDetails[i] = Control(L"STATIC", Tr(detail), SS_ENDELLIPSIS);
            auto& button = panel.generalButtons[i];
            button = Control(L"BUTTON", Tr(title), BS_OWNERDRAW | WS_TABSTOP, id);
            if (!panel.generalTitles[i] || !panel.generalDetails[i] || !button) return -1;
            SetWindowSubclass(button, SettingsControlProc, 1, 0);
        }
        panel.status = Control(L"STATIC", L"", WS_VISIBLE | SS_NOPREFIX);
        panel.languageTitle = Control(L"STATIC", Tr(Text::languageTitle), SS_ENDELLIPSIS);
        panel.languageHint = Control(L"STATIC", Tr(Text::languageRestartHint), SS_ENDELLIPSIS);
        panel.languageCombo = Control(L"COMBOBOX", L"", CBS_DROPDOWNLIST | WS_TABSTOP, languageChoice);
        panel.languageApply = Control(L"BUTTON", Tr(Text::apply), BS_OWNERDRAW | WS_TABSTOP, applyLanguage);
        if (!panel.languageTitle || !panel.languageHint || !panel.languageCombo || !panel.languageApply) return -1;
        SetWindowSubclass(panel.languageApply, SettingsControlProc, 1, 0);
        for (const auto text : languageLabels) {
            SendMessageW(panel.languageCombo, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(Tr(text)));
        }
        panel.configPathText = Control(L"STATIC", L"", SS_PATHELLIPSIS);
        panel.configHelp = Control(L"STATIC", Tr(Text::configurationHelp), SS_NOPREFIX);
        panel.configSelect = Control(L"BUTTON", Tr(Text::changeLocation), BS_OWNERDRAW | WS_TABSTOP, selectConfig);
        panel.configImport = Control(L"BUTTON", Tr(Text::importConfiguration), BS_OWNERDRAW | WS_TABSTOP, importConfig);
        panel.configExport = Control(L"BUTTON", Tr(Text::exportConfiguration), BS_OWNERDRAW | WS_TABSTOP, exportConfig);
        if (!panel.configPathText || !panel.configHelp || !panel.configSelect || !panel.configImport || !panel.configExport) return -1;
        panel.engineTransferTitle = Control(L"STATIC", Tr(Text::engineTransfer));
        panel.engineImport = Control(L"BUTTON", Tr(Text::importSearchEngines), BS_OWNERDRAW | WS_TABSTOP, importEngines);
        panel.engineExport = Control(L"BUTTON", Tr(Text::exportSearchEngines), BS_OWNERDRAW | WS_TABSTOP, exportEngines);
        panel.clipboardTitle = Control(L"STATIC", Tr(Text::clipboardSearch));
        panel.clipboardHint = Control(L"STATIC", L"", SS_ENDELLIPSIS);
        panel.clipboardRecord = Control(L"BUTTON", Tr(Text::recordClipboardShortcut), BS_OWNERDRAW | WS_TABSTOP, recordClipboardShortcut);
        panel.clipboardClear = Control(L"BUTTON", Tr(Text::clear), BS_OWNERDRAW | WS_TABSTOP, clearClipboardShortcut);
        panel.excludedTitle = Control(L"STATIC", Tr(Text::doubleAltExclusions));
        panel.excludedApps = Control(L"EDIT", L"", ES_AUTOHSCROLL | WS_TABSTOP, excludedAppList);
        panel.excludedSave = Control(L"BUTTON", Tr(Text::apply), BS_OWNERDRAW | WS_TABSTOP, applyExcludedApps);
        if (!panel.engineTransferTitle || !panel.clipboardTitle || !panel.clipboardHint || !panel.excludedTitle) return -1;
        for (const auto control : {panel.configSelect, panel.configImport, panel.configExport, panel.engineImport, panel.engineExport,
            panel.clipboardRecord, panel.clipboardClear, panel.excludedApps, panel.excludedSave}) {
            if (!control) return -1;
            SetWindowSubclass(control, SettingsControlProc, 1, 0);
        }
        SendMessageW(panel.excludedApps, EM_LIMITTEXT, 8192, 0);
        SendMessageW(panel.excludedApps, EM_SETCUEBANNER, TRUE, reinterpret_cast<LPARAM>(Tr(Text::appExclusionsExample)));
        for (HWND control : {panel.nav, panel.title, panel.section, panel.fontHeading, panel.fontName,
            panel.fileHeading, panel.fontPath, panel.previewHeading, panel.preview, panel.systemButton,
            panel.fileButton, panel.resetButton, panel.engineMenuTitle, panel.engineMenuLabels, panel.engineMenuIcons,
            panel.engines, panel.addEngine, panel.nameLabel, panel.name,
            panel.urlLabel, panel.url, panel.urlHint, panel.iconLabel, panel.iconColor, panel.iconFile, panel.cancel,
            panel.save, panel.status, panel.shortcutTitle, panel.shortcutHint, panel.shortcutRecord, panel.shortcutSave,
            panel.shortcutCancel, panel.shortcutReset, panel.selectionTitle, panel.selectionHint, panel.selectionEngines,
            panel.selectionMode, panel.selectionRecord, panel.selectionClear, panel.browserSelect, panel.browsers, panel.browserOther, panel.browserPath,
            panel.browserBrowse, panel.browserSave, panel.browserCancel, panel.aliasLabel, panel.alias, panel.engineEdit,
            panel.engineDelete, panel.engineUp, panel.engineDown, panel.engineDefault, panel.enginePin, panel.engineTest, panel.history,
            panel.historyToggle, panel.historyMore, panel.historyEmpty, panel.historyKeyword, panel.historySource}) {
            if (!control) return -1;
        }
        for (HWND control : {panel.nav, panel.systemButton, panel.fileButton, panel.resetButton,
            panel.engineMenuLabels, panel.engineMenuIcons, panel.addEngine,
            panel.name, panel.url, panel.iconColor, panel.iconFile, panel.cancel, panel.save,
            panel.shortcutRecord, panel.shortcutSave, panel.shortcutCancel, panel.shortcutReset, panel.selectionEngines,
            panel.selectionMode, panel.selectionRecord, panel.selectionClear, panel.browserSelect,
            panel.browserOther, panel.browsers, panel.browserPath, panel.browserBrowse, panel.browserSave, panel.browserCancel,
            panel.alias, panel.engineEdit, panel.engineDelete, panel.engineUp, panel.engineDown, panel.engineDefault, panel.enginePin,
            panel.engineTest, panel.history, panel.historyToggle, panel.historyMore, panel.historyKeyword, panel.historySource}) {
            SetWindowSubclass(control, SettingsControlProc, 1, 0);
        }

        for (const auto page : pageNames) ListBox_AddString(panel.nav, Tr(page));
        for (const auto* mode : {Tr(Text::searchDirectly), Tr(Text::openSearchBox)}) {
            SendMessageW(panel.selectionMode, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(mode));
        }
        SendMessageW(panel.selectionMode, CB_SETCURSEL, 0, 0);
        SendMessageW(panel.historyKeyword, EM_LIMITTEXT, 256, 0);
        SendMessageW(panel.historySource, EM_LIMITTEXT, 256, 0);
        SendMessageW(panel.historyKeyword, EM_SETCUEBANNER, TRUE, reinterpret_cast<LPARAM>(Tr(Text::searchTerms)));
        SendMessageW(panel.historySource, EM_SETCUEBANNER, TRUE, reinterpret_cast<LPARAM>(Tr(Text::sourceApp)));
        ListBox_SetCurSel(panel.nav, generalPage);
        RefreshEngineList();
        SendMessageW(panel.name, EM_LIMITTEXT, engineNameLimit, 0);
        SendMessageW(panel.url, EM_LIMITTEXT, engineUrlLimit, 0);
        SendMessageW(panel.alias, EM_LIMITTEXT, engineAliasLimit + 1, 0);
        SendMessageW(panel.alias, EM_SETCUEBANNER, FALSE, reinterpret_cast<LPARAM>(Tr(Text::aliasExample)));
        SendMessageW(panel.browserPath, EM_LIMITTEXT, 32767, 0);
        SendMessageW(panel.browserPath, EM_SETCUEBANNER, FALSE, reinterpret_cast<LPARAM>(Tr(Text::browserPathHint)));
        SendMessageW(panel.name, EM_SETCUEBANNER, FALSE, reinterpret_cast<LPARAM>(Tr(Text::engineNameExample)));
        SendMessageW(panel.url, EM_SETCUEBANNER, FALSE, reinterpret_cast<LPARAM>(L"https://zh.wikipedia.org/wiki/Special:Search?search=%s"));
        RefreshWindowFonts();
        ShowWindow(panel.nav, SW_SHOW);
        ShowPage(generalPage);
        return 0;
    case WM_MEASUREITEM:
        if (key == navigation || key == engineList || key == browserList || key == historyList) {
            reinterpret_cast<MEASUREITEMSTRUCT*>(data)->itemHeight =
                Px(key == navigation ? navRowHeight : key == engineList ? engineRowHeight : key == browserList ? browserRowHeight : historyRowHeight);
            return TRUE;
        }
        break;
    case WM_DRAWITEM: {
        const auto& item = *reinterpret_cast<const DRAWITEMSTRUCT*>(data);
        if (item.CtlID == navigation) {
            PaintNavigation(item);
            return TRUE;
        }
        if (item.CtlID == engineList) {
            PaintEngineRow(item);
            return TRUE;
        }
        if (item.CtlID == browserList) {
            PaintBrowserRow(item);
            return TRUE;
        }
        if (item.CtlID == historyList) {
            PaintHistoryRow(item);
            return TRUE;
        }
        if (item.CtlType == ODT_BUTTON) {
            PaintButton(item);
            return TRUE;
        }
        break;
    }
    case WM_COMMAND:
        switch (LOWORD(key)) {
        case languageChoice:
            if (HIWORD(key) == CBN_SELCHANGE) UpdateLanguageChoice();
            return 0;
        case applyLanguage:
            if (HIWORD(key) == BN_CLICKED) ApplyLanguageChoice();
            return 0;
        case engineMenuWithLabels:
        case engineMenuIconsOnly:
            if (HIWORD(key) == BN_CLICKED) {
                const bool saved = panel.application->changeEngineTileNames(LOWORD(key) == engineMenuWithLabels);
                SetWindowTextW(panel.status, saved ? L"" : Tr(Text::cannotSaveSetting));
                InvalidateRect(panel.engineMenuLabels, nullptr, FALSE);
                InvalidateRect(panel.engineMenuIcons, nullptr, FALSE);
            }
            return 0;
        case selectConfig:
        case importConfig:
        case exportConfig:
        case importEngines:
        case exportEngines:
            if (HIWORD(key) == BN_CLICKED) ManageConfiguration(LOWORD(key));
            return 0;
        case startWithWindows:
        case showOnLaunch:
        case closeToTray:
            if (HIWORD(key) == BN_CLICKED) ToggleGeneralOption(static_cast<ControlId>(LOWORD(key)));
            return 0;
        case navigation:
            if (HIWORD(key) == LBN_SELCHANGE) {
                const int page = ListBox_GetCurSel(panel.nav);
                if (page != LB_ERR) ShowPage(page);
            }
            return 0;
        case systemFont:
            if (HIWORD(key) == BN_CLICKED) PickSystemFont();
            return 0;
        case localFont:
            if (HIWORD(key) == BN_CLICKED) PickFontFile();
            return 0;
        case resetFont:
            if (HIWORD(key) == BN_CLICKED) ChangeFont({});
            return 0;
        case engineList:
            if (HIWORD(key) == LBN_SELCHANGE) UpdateEngineControls();
            if (HIWORD(key) == LBN_DBLCLK) ManageEngine(defaultEngine);
            return 0;
        case defaultEngine:
        case pinEngine:
        case editEngine:
        case deleteEngine:
        case moveEngineUp:
        case moveEngineDown:
            if (HIWORD(key) == BN_CLICKED) ManageEngine(LOWORD(key));
            return 0;
        case testEngine:
            if (HIWORD(key) == BN_CLICKED) {
                auto draft = panel.draft;
                draft.url = ReadControl(panel.url);
                const auto result = panel.application->testEngine(draft, Tr(Text::appName));
                const auto success = i18n::Format(Text::searchTestLaunched, {Tr(Text::appName)});
                SetWindowTextW(panel.status, result == SearchResult::opened ? success.c_str()
                    : result == SearchResult::emptyQuery ? Tr(Text::enterSearchUrl) : Tr(Text::cannotOpenBrowser));
            }
            return 0;
        case toggleHistory:
            if (HIWORD(key) == BN_CLICKED) {
                const bool enabled = !panel.application->preferences().recordHistory;
                if (panel.application->changeHistoryRecording(enabled)) {
                    SetWindowTextW(panel.historyToggle, enabled ? Tr(Text::historyOn) : Tr(Text::historyOff));
                    InvalidateRect(panel.historyToggle, nullptr, FALSE);
                    SetWindowTextW(panel.status, enabled ? Tr(Text::historyEnabled) : Tr(Text::historyDisabled));
                } else SetWindowTextW(panel.status, Tr(Text::cannotSaveHistorySetting));
            }
            return 0;
        case moreHistory:
            if (HIWORD(key) == BN_CLICKED) {
                if (panel.historyFilterPending) {
                    RefreshHistory();
                    return 0;
                }
                const int first = static_cast<int>(panel.historyRows.size());
                RefreshHistory(true);
                if (first < static_cast<int>(panel.historyRows.size())) {
                    ListBox_SetCurSel(panel.history, first);
                    ListBox_SetTopIndex(panel.history, first);
                }
                SetFocus(panel.history);
            }
            return 0;
        case historyKeyword:
        case historySource:
            if (HIWORD(key) == EN_CHANGE && panel.page == historyPage) {
                panel.historyFilterPending = true;
                if (!SetTimer(window, historyFilterTimer, 180, nullptr)) RefreshHistory();
            }
            return 0;
        case newEngine:
            if (HIWORD(key) == BN_CLICKED) StartEngineEditor();
            return 0;
        case chooseIconColor:
            if (HIWORD(key) == BN_CLICKED) PickIconColor();
            return 0;
        case chooseIconFile:
            if (HIWORD(key) == BN_CLICKED) PickIconFile();
            return 0;
        case saveEngine:
            if (HIWORD(key) == BN_CLICKED) SaveEngineDraft();
            return 0;
        case cancelEngine:
            if (HIWORD(key) == BN_CLICKED) CloseEngineEditor();
            return 0;
        case recordShortcut:
        case recordSelectionShortcut:
        case recordClipboardShortcut:
            if (HIWORD(key) == BN_CLICKED) {
                SetShortcutRecording(reinterpret_cast<HWND>(data));
                SetWindowTextW(panel.status, L"");
            }
            return 0;
        case saveShortcut:
            if (HIWORD(key) == BN_CLICKED && panel.shortcutDraft) {
                if (panel.recording == panel.selectionRecord) {
                    const auto* engine = SelectedShortcutEngine();
                    const bool preview = SendMessageW(panel.selectionMode, CB_GETCURSEL, 0, 0) == 1;
                    if (engine && ApplySelectionBinding(SelectionShortcut{*panel.shortcutDraft, engine->id, preview})) {
                        SetShortcutRecording();
                        SetFocus(panel.selectionRecord);
                    }
                } else if (panel.recording == panel.clipboardRecord) ApplyClipboardBinding(panel.shortcutDraft);
                else ApplyShortcut(*panel.shortcutDraft);
            }
            return 0;
        case cancelShortcut:
            if (HIWORD(key) == BN_CLICKED) {
                const auto control = panel.recording;
                SetShortcutRecording();
                SetWindowTextW(panel.status, L"");
                SetFocus(control);
            }
            return 0;
        case resetShortcut:
            if (HIWORD(key) == BN_CLICKED) ApplyShortcut({});
            return 0;
        case selectionEngineList:
        case selectionMode:
            if (HIWORD(key) == CBN_SELENDOK
                || (HIWORD(key) == CBN_SELCHANGE && !SendMessageW(reinterpret_cast<HWND>(data), CB_GETDROPPEDSTATE, 0, 0))) {
                const auto& binding = panel.application->preferences().selectionShortcut;
                const auto* engine = SelectedShortcutEngine();
                if (binding && engine) {
                    const bool preview = SendMessageW(panel.selectionMode, CB_GETCURSEL, 0, 0) == 1;
                    const SelectionShortcut updated{binding->shortcut, engine->id, preview};
                    if (*binding != updated && !ApplySelectionBinding(updated)) RefreshSelectionEngines();
                }
            }
            return 0;
        case clearSelectionShortcut:
            if (HIWORD(key) == BN_CLICKED && ApplySelectionBinding(std::nullopt)) SetFocus(panel.selectionRecord);
            return 0;
        case clearClipboardShortcut:
            if (HIWORD(key) == BN_CLICKED) ApplyClipboardBinding(std::nullopt);
            return 0;
        case applyExcludedApps:
            if (HIWORD(key) == BN_CLICKED) SaveExcludedApps();
            return 0;
        case selectBrowser:
            if (HIWORD(key) == BN_CLICKED) {
                if (panel.browserPanel == BrowserPanel::closed) {
                    RefreshBrowsers();
                    SetBrowserPanel(BrowserPanel::list);
                    SetFocus(panel.browsers);
                } else SetBrowserPanel(BrowserPanel::closed);
                SetWindowTextW(panel.status, L"");
            }
            return 0;
        case otherBrowser:
            if (HIWORD(key) == BN_CLICKED) {
                SetBrowserPanel(BrowserPanel::path);
                SetWindowTextW(panel.browserPath, panel.application->preferences().browser.c_str());
                SetWindowTextW(panel.status, L"");
                SetFocus(panel.browserPath);
                SendMessageW(panel.browserPath, EM_SETSEL, 0, -1);
            }
            return 0;
        case browseBrowser:
            if (HIWORD(key) == BN_CLICKED) BrowseForBrowser();
            return 0;
        case saveBrowser:
            if (HIWORD(key) == BN_CLICKED) SaveBrowserSelection(ReadControl(panel.browserPath), true);
            return 0;
        case cancelBrowser:
            if (HIWORD(key) == BN_CLICKED) {
                SetBrowserPanel(BrowserPanel::list);
                SetWindowTextW(panel.status, L"");
                SetFocus(panel.browsers);
            }
            return 0;
        }
        break;
    case WM_SIZE:
        if (panel.status) Layout();
        return 0;
    case WM_VSCROLL: {
        SCROLLINFO scroll{sizeof(scroll), SIF_ALL};
        if (!GetScrollInfo(window, SB_VERT, &scroll)) return 0;
        switch (LOWORD(key)) {
        case SB_LINEUP: panel.scroll -= Px(22); break;
        case SB_LINEDOWN: panel.scroll += Px(22); break;
        case SB_PAGEUP: panel.scroll -= static_cast<int>(scroll.nPage); break;
        case SB_PAGEDOWN: panel.scroll += static_cast<int>(scroll.nPage); break;
        case SB_THUMBPOSITION:
        case SB_THUMBTRACK: panel.scroll = scroll.nTrackPos; break;
        case SB_TOP: panel.scroll = scroll.nMin; break;
        case SB_BOTTOM: panel.scroll = scroll.nMax; break;
        default: return 0;
        }
        Layout();
        return 0;
    }
    case WM_MOUSEWHEEL:
        panel.scroll -= MulDiv(GET_WHEEL_DELTA_WPARAM(key), Px(44), WHEEL_DELTA);
        Layout();
        return 0;
    case WM_TIMER:
        if (key == historyFilterTimer && panel.historyFilterPending && panel.page == historyPage) {
            RefreshHistory();
            return 0;
        }
        break;
    case WM_GETMINMAXINFO: {
        auto& limits = *reinterpret_cast<MINMAXINFO*>(data);
        limits.ptMinTrackSize = {Px(667), Px(345)};
        return 0;
    }
    case WM_DPICHANGED: {
        panel.dpi = LOWORD(key);
        const auto& bounds = *reinterpret_cast<const RECT*>(data);
        RefreshWindowFonts();
        SetWindowPos(window, nullptr, bounds.left, bounds.top, bounds.right - bounds.left,
            bounds.bottom - bounds.top, SWP_NOZORDER | SWP_NOACTIVATE);
        Layout();
        return 0;
    }
    case WM_CTLCOLORSTATIC: {
        const auto dc = reinterpret_cast<HDC>(key);
        const auto control = reinterpret_cast<HWND>(data);
        const bool generalDetail = std::ranges::find(panel.generalDetails, control) != panel.generalDetails.end();
        const bool inCard = generalDetail || control == panel.languageTitle || control == panel.languageHint
            || control == panel.configPathText || control == panel.configHelp
            || control == panel.engineTransferTitle || control == panel.clipboardTitle || control == panel.clipboardHint
            || control == panel.excludedTitle
            || std::ranges::find(panel.generalTitles, control) != panel.generalTitles.end()
            || control == panel.fontHeading || control == panel.fontName || control == panel.fileHeading
            || control == panel.fontPath || control == panel.previewHeading || control == panel.preview || control == panel.engineMenuTitle
            || control == panel.nameLabel || control == panel.urlLabel || control == panel.urlHint || control == panel.iconLabel
            || control == panel.shortcutTitle || control == panel.shortcutHint || control == panel.aliasLabel
            || control == panel.selectionTitle || control == panel.selectionHint;
        const bool secondary = generalDetail || control == panel.languageHint || control == panel.configHelp || control == panel.clipboardHint
            || control == panel.fontName || control == panel.fontPath || control == panel.urlHint
            || control == panel.status || control == panel.section || control == panel.shortcutHint
            || control == panel.selectionHint || control == panel.historyEmpty;
        SetBkColor(dc, inCard ? panel.colors.card : panel.colors.surface);
        SetTextColor(dc, secondary ? panel.colors.secondary : panel.colors.text);
        return reinterpret_cast<LRESULT>(inCard ? panel.cardBrush : panel.surfaceBrush);
    }
    case WM_CTLCOLOREDIT: {
        const auto dc = reinterpret_cast<HDC>(key);
        SetBkColor(dc, panel.colors.button);
        SetTextColor(dc, panel.colors.text);
        const auto brush = GetStockObject(DC_BRUSH);
        SetDCBrushColor(dc, panel.colors.button);
        return reinterpret_cast<LRESULT>(brush);
    }
    case WM_CTLCOLORLISTBOX:
        if (reinterpret_cast<HWND>(data) == panel.nav || reinterpret_cast<HWND>(data) == panel.history) {
            const auto dc = reinterpret_cast<HDC>(key);
            SetBkColor(dc, panel.colors.surface);
            SetTextColor(dc, panel.colors.text);
            return reinterpret_cast<LRESULT>(panel.surfaceBrush);
        }
        if (reinterpret_cast<HWND>(data) == panel.engines || reinterpret_cast<HWND>(data) == panel.browsers) {
            return reinterpret_cast<LRESULT>(panel.cardBrush);
        }
        break;
    case WM_SYSCOLORCHANGE:
    case WM_THEMECHANGED:
        RefreshPalette();
        RedrawWindow(window, nullptr, nullptr, RDW_INVALIDATE | RDW_ALLCHILDREN);
        break;
    case WM_ERASEBKGND:
        return 1;
    case WM_PAINT: {
        PAINTSTRUCT paint{};
        const HDC dc = BeginPaint(window, &paint);
        PaintSettings(window, dc, paint.rcPaint);
        EndPaint(window, &paint);
        return 0;
    }
    case WM_NCDESTROY: {
        const auto scheduleCleanup = panel.actions.scheduleGraphicsCleanup;
        const HWND owner = GetWindow(window, GW_OWNER);
        if (panel.recording && panel.actions.recordShortcut) panel.actions.recordShortcut(false);
        if (panel.headerFont) DeleteObject(panel.headerFont);
        if (panel.bodyFont) DeleteObject(panel.bodyFont);
        if (panel.detailFont) DeleteObject(panel.detailFont);
        if (panel.previewFont) DeleteObject(panel.previewFont);
        if (panel.surfaceBrush) DeleteObject(panel.surfaceBrush);
        if (panel.cardBrush) DeleteObject(panel.cardBrush);
        panel = {};
        if (scheduleCleanup && owner) scheduleCleanup(owner);
        break;
    }
    }
    return DefWindowProcW(window, message, key, data);
}
}

HWND CreateSettings(HWND owner, Application& application, ID2D1Factory& factory, const SettingsValues& values, SettingsActions actions)
{
    if (panel.window) return panel.window;

    const HINSTANCE instance = GetModuleHandleW(nullptr);
    WNDCLASSW windowClass{};
    windowClass.hInstance = instance;
    windowClass.lpszClassName = className;
    windowClass.lpfnWndProc = SettingsProc;
    windowClass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    if (!RegisterClassW(&windowClass) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) return nullptr;

    MONITORINFO monitor{sizeof(monitor)};
    if (!GetMonitorInfoW(MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST), &monitor)) return nullptr;
    panel.factory = &factory;
    panel.values = values;
    panel.actions = actions;
    panel.application = &application;
    panel.dpi = GetDpiForWindow(owner);
    constexpr DWORD style = WS_OVERLAPPEDWINDOW | WS_VSCROLL | WS_CLIPCHILDREN;
    RECT bounds{0, 0, Px(788), Px(482)};
    AdjustWindowRectExForDpi(&bounds, style, FALSE, WS_EX_CONTROLPARENT, panel.dpi);
    const auto& work = monitor.rcWork;
    const int width = std::min<int>(bounds.right - bounds.left, work.right - work.left);
    const int height = std::min<int>(bounds.bottom - bounds.top, work.bottom - work.top);
    const HWND window = CreateWindowExW(WS_EX_CONTROLPARENT, className, Tr(Text::settingsWindowTitle), style,
        work.left + std::max(0L, (work.right - work.left - width) / 2),
        work.top + std::max(0L, (work.bottom - work.top - height) / 2),
        width, height, owner, nullptr, instance, nullptr);
    if (!window) {
        panel = {};
        return nullptr;
    }
    constexpr DWM_WINDOW_CORNER_PREFERENCE corners = DWMWCP_ROUND;
    DwmSetWindowAttribute(window, DWMWA_WINDOW_CORNER_PREFERENCE, &corners, sizeof(corners));
    return window;
}

void ShowSettings(HWND owner, Application& application, ID2D1Factory& factory, const SettingsValues& values, SettingsActions actions)
{
    if (panel.window) {
        if (IsIconic(panel.window)) ShowWindow(panel.window, SW_RESTORE);
        SetForegroundWindow(panel.window);
        return;
    }
    const HWND window = CreateSettings(owner, application, factory, values, actions);
    if (!window) return;
    ShowWindow(window, SW_SHOW);
    SetFocus(panel.nav);
}

bool HasOpenSettings()
{
    return panel.window != nullptr;
}

bool HandleSettingsMessage(MSG& message)
{
    if (!panel.window || (message.hwnd != panel.window && !IsChild(panel.window, message.hwnd))) return false;
    if (HandleShortcutRecording(message)) return true;
    if (message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE) {
        if (SendMessageW(panel.selectionEngines, CB_GETDROPPEDSTATE, 0, 0)
            || SendMessageW(panel.selectionMode, CB_GETDROPPEDSTATE, 0, 0)) return false;
        if (panel.recording) {
            const auto control = panel.recording;
            SetShortcutRecording();
            SetWindowTextW(panel.status, L"");
            SetFocus(control);
        } else if (panel.browserPanel != BrowserPanel::closed) {
            SetBrowserPanel(BrowserPanel::closed);
            SetFocus(panel.browserSelect);
        } else if (panel.editing) CloseEngineEditor();
        else DestroyWindow(panel.window);
        return true;
    }
    if (message.message == WM_KEYDOWN && message.wParam == VK_RETURN && message.hwnd == panel.browserPath) {
        SaveBrowserSelection(ReadControl(panel.browserPath), true);
        return true;
    }
    if (message.message == WM_KEYDOWN && message.wParam == VK_RETURN
        && (message.hwnd == panel.historyKeyword || message.hwnd == panel.historySource)) {
        RefreshHistory();
        return true;
    }
    if (message.message == WM_KEYDOWN && message.wParam == VK_RETURN && message.hwnd == panel.excludedApps) {
        SaveExcludedApps();
        return true;
    }
    if (message.message == WM_KEYDOWN && (message.wParam == VK_RETURN || message.wParam == VK_SPACE)
        && message.hwnd == panel.browsers) {
        ApplyBrowserRow(ListBox_GetCurSel(panel.browsers));
        return true;
    }
    if (message.message == WM_KEYDOWN && message.hwnd == panel.history
        && (message.wParam == VK_RETURN || message.wParam == VK_DELETE)) {
        UseHistoryRow(ListBox_GetCurSel(panel.history), message.wParam == VK_DELETE);
        return true;
    }
    if (message.message == WM_KEYDOWN && message.hwnd == panel.engines && message.wParam == VK_RETURN) {
        ManageEngine(defaultEngine);
        return true;
    }
    return IsDialogMessageW(panel.window, &message) != FALSE;
}

void UpdateSettingsEngine()
{
    if (!panel.window) return;
    SelectEngine();
    UpdateEngineControls();
    InvalidateRect(panel.engines, nullptr, FALSE);
}

void ShowEngineSettingsPage(bool add)
{
    if (!panel.window) return;
    ListBox_SetCurSel(panel.nav, enginePage);
    if (add && !(panel.page == enginePage && panel.editing && panel.draft.id.empty())) StartEngineEditor();
    if (panel.page != enginePage) ShowPage(enginePage);
    SetFocus(panel.editing ? panel.name : panel.engines);
}

void ShowConfigurationSettingsPage()
{
    if (!panel.window) return;
    ListBox_SetCurSel(panel.nav, configurationPage);
    if (panel.page != configurationPage) ShowPage(configurationPage);
    SetFocus(panel.nav);
}

bool CaptureRegisteredShortcut(Shortcut shortcut)
{
    if (!panel.recording) return false;
    if (GetFocus() == panel.recording) PreviewShortcut(shortcut);
    return true;
}

}
