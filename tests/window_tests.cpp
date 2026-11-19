#include "check.h"
#include "search_cases.h"
#include "test_registry.h"

// Test the private UI state and Win32 handlers without showing or activating a window.
#include "../src/ui/search_window.cpp"

#include <filesystem>
#include <iostream>

namespace {
using namespace yisou;
using namespace yisou::ui;

std::wstring openedUrl;
std::wstring openedBrowser;
bool allowSave = true;
bool allowOpen = true;

bool Open(const std::wstring& url, const std::wstring& browser)
{
    openedUrl = url;
    openedBrowser = browser;
    return allowOpen;
}

struct SearchWindow {
    HWND window = nullptr;

    ~SearchWindow()
    {
        if (window) DestroyWindow(window);
        ReleaseSearchResources();
    }
};

LRESULT HitClientPoint(HWND window, POINT point)
{
    CHECK(ClientToScreen(window, &point));
    return SendMessageW(window, WM_NCHITTEST, 0, MAKELPARAM(point.x, point.y));
}

LRESULT HitControl(HWND window, HWND control)
{
    RECT bounds{};
    CHECK(GetWindowRect(control, &bounds));
    return SendMessageW(window, WM_NCHITTEST, 0,
        MAKELPARAM((bounds.left + bounds.right) / 2, (bounds.top + bounds.bottom) / 2));
}

void Notify(HWND window, HWND control, WORD notification)
{
    SendMessageW(window, WM_COMMAND, MAKEWPARAM(GetDlgCtrlID(control), notification), reinterpret_cast<LPARAM>(control));
}

HWND FindOwnedSettings(HWND owner)
{
    HWND settings = nullptr;
    while ((settings = FindWindowExW(nullptr, settings, L"YiSouSettings", nullptr))) {
        if (GetWindow(settings, GW_OWNER) == owner) return settings;
    }
    return nullptr;
}

void CheckSearchPlaceholder(std::wstring_view expected)
{
    wchar_t placeholder[64]{};
    CHECK(SendMessageW(edit, EM_GETCUEBANNER, reinterpret_cast<WPARAM>(placeholder), std::size(placeholder)));
    CHECK(std::wstring_view(placeholder) == expected);
}

void CheckIconBlit(unsigned edge)
{
    const auto pixels = TablerPixels(TablerIcon::chevronDown, RGB(83, 98, 115), edge);
    CHECK(pixels && pixels->size() == edge * edge);
    ComPtr<ID2D1DeviceContext> canvas;
    CHECK(SUCCEEDED(drawingDevice->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE, canvas.GetAddressOf())));
    ComPtr<ID2D1Bitmap> icon;
    CreateIconBitmap(*canvas.Get(), *pixels, edge, icon);
    CHECK(icon);

    const auto format = D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED);
    const auto targetProperties = D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_TARGET, format, 96, 96);
    const auto readbackProperties = D2D1::BitmapProperties1(
        D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW, format, 96, 96);
    const auto size = D2D1::SizeU(edge + 20, edge + 20);
    ComPtr<ID2D1Bitmap1> target, readback;
    CHECK(SUCCEEDED(canvas->CreateBitmap(size, nullptr, 0, &targetProperties, target.GetAddressOf())));
    CHECK(SUCCEEDED(canvas->CreateBitmap(size, nullptr, 0, &readbackProperties, readback.GetAddressOf())));

    canvas->SetTarget(target.Get());
    canvas->SetDpi(96, 96);
    canvas->SetTransform(D2D1::Matrix3x2F::Translation(5, 7));
    canvas->BeginDraw();
    canvas->Clear(D2D1::ColorF(0, 0, 0, 0));
    DrawIconBitmap(*canvas.Get(), *icon.Get(), 3, 4, edge);
    CHECK(SUCCEEDED(canvas->EndDraw()));
    canvas->SetTarget(nullptr);
    CHECK(SUCCEEDED(readback->CopyFromBitmap(nullptr, target.Get(), nullptr)));
    D2D1_MAPPED_RECT mapped{};
    CHECK(SUCCEEDED(readback->Map(D2D1_MAP_OPTIONS_READ, &mapped)));
    bool matches = true;
    for (unsigned y = 0; y < size.height; ++y) {
        const auto* row = reinterpret_cast<const std::uint32_t*>(mapped.bits + y * mapped.pitch);
        for (unsigned x = 0; x < size.width; ++x) {
            const auto expected = x >= 8 && x < 8 + edge && y >= 11 && y < 11 + edge
                ? (*pixels)[(y - 11) * edge + x - 8] : 0u;
            matches &= row[x] == expected;
        }
    }
    readback->Unmap();
    CHECK(matches);
}

void CheckSvgRasterizer()
{
    constexpr auto wide = R"(<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12" viewBox="0 0 24 12"><rect width="24" height="12" fill="red"/></svg>)";
    const auto fitted = RasterizeSvgIcon(wide, 24);
    CHECK(fitted.size() == 576 && fitted[0] == 0 && fitted[12 * 24] == 0xffff0000);
    const auto layers = RasterizeSvgIcon(
        R"(<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><path d="M0 0H1L0 1Z" fill="red"/><path d="M1 0V1H0Z" fill="blue"/></svg>)", 1);
    CHECK(layers.size() == 1 && (layers[0] >> 24) >= 248);
    CHECK(RasterizeSvgIcon(wide, 0).empty());
    CHECK(RasterizeSvgIcon(wide, maxSvgIconSize + 1).empty());
    CHECK(RasterizeSvgIcon("<broken", 24).empty());
    CHECK(RasterizeSvgIcon(std::string(svgIconLimit + 1, 'x'), 24).empty());

    const auto blue = TablerPixels(TablerIcon::chevronDown, RGB(0, 95, 184), 20);
    CHECK(blue && blue == TablerPixels(TablerIcon::chevronDown, RGB(0, 95, 184), 20));
    CHECK(blue != TablerPixels(TablerIcon::chevronDown, RGB(255, 0, 0), 20));
    CHECK(blue != TablerPixels(TablerIcon::chevronDown, RGB(0, 95, 184), 25));
    const auto original = *blue;
    const auto leastRecent = TablerPixels(TablerIcon::chevronUp, RGB(0, 95, 184), 20);
    for (unsigned index = 1; index <= 12; ++index) {
        CHECK(blue == TablerPixels(TablerIcon::chevronDown, RGB(0, 95, 184), 20));
        CHECK(TablerPixels(TablerIcon::chevronDown, RGB(index, 40, 90), 224));
    }
    CHECK(*blue == original);
    CHECK(blue == TablerPixels(TablerIcon::chevronDown, RGB(0, 95, 184), 20));
    CHECK(leastRecent != TablerPixels(TablerIcon::chevronUp, RGB(0, 95, 184), 20));
    Engine invalid;
    invalid.svgIcon = "<broken";
    CHECK(EngineIcon(invalid, 24));
    CHECK(EngineIcon(invalid, 24) == EngineIcon(Engine{}, 24));
}

void CheckHarmonyFont(const LOGFONTW& face)
{
    wchar_t directWriteName[LF_FACESIZE]{};
    CHECK(SUCCEEDED(engineFormat->GetFontFamilyName(directWriteName, LF_FACESIZE)));
    CHECK(std::wstring_view(directWriteName) == L"HarmonyOS Sans SC");

    const HDC dc = CreateCompatibleDC(nullptr);
    CHECK(dc);
    const HGDIOBJ previous = SelectObject(dc, editFont);
    wchar_t gdiName[LF_FACESIZE]{};
    GetTextFaceW(dc, LF_FACESIZE, gdiName);
    WORD glyph = 0xffff;
    const DWORD glyphCount = GetGlyphIndicesW(dc, L"中", 1, &glyph, GGI_MARK_NONEXISTING_GLYPHS);
    SelectObject(dc, previous);
    DeleteDC(dc);
    CHECK(std::wstring_view(gdiName) == face.lfFaceName);
    CHECK(glyphCount == 1 && glyph != 0xffff);
}

void CheckFlyoutPaint(HWND flyout, HWND selected, HWND other)
{
    RECT bounds{};
    CHECK(GetClientRect(flyout, &bounds));
    const HDC dc = CreateCompatibleDC(nullptr);
    const HDC reference = GetDC(nullptr);
    const HBITMAP bitmap = CreateCompatibleBitmap(reference, bounds.right, bounds.bottom);
    ReleaseDC(nullptr, reference);
    CHECK(dc && bitmap);
    const HGDIOBJ previous = SelectObject(dc, bitmap);
    SendMessageW(flyout, WM_PRINTCLIENT, reinterpret_cast<WPARAM>(dc), PRF_CLIENT);
    const auto paint = [&](HWND tile) {
        DRAWITEMSTRUCT item{};
        item.CtlType = ODT_BUTTON;
        item.CtlID = GetDlgCtrlID(tile);
        item.itemAction = ODA_DRAWENTIRE;
        item.hwndItem = tile;
        item.hDC = dc;
        CHECK(GetWindowRect(tile, &item.rcItem));
        MapWindowPoints(nullptr, flyout, reinterpret_cast<POINT*>(&item.rcItem), 2);
        SendMessageW(flyout, WM_DRAWITEM, item.CtlID, reinterpret_cast<LPARAM>(&item));
        const int x = (item.rcItem.left + item.rcItem.right) / 2;
        const int y = item.rcItem.bottom - MulDiv(8, GetDpiForWindow(flyout), USER_DEFAULT_SCREEN_DPI);
        return GetPixel(dc, x, y);
    };
    const auto selection = paint(selected);
    const auto unselected = paint(other);
    SelectObject(dc, previous);
    DeleteObject(bitmap);
    DeleteDC(dc);
    CHECK(selection != CLR_INVALID && unselected != CLR_INVALID && selection != unselected);
}

}

int wmain(int argc, wchar_t** argv) try
{
    using namespace yisou;
    using namespace yisou::ui;
    CHECK(argc == 4);
    i18n::SetLanguage(Language::simplifiedChinese);
    const FontChoice missing{FontSource::file, std::wstring(argv[1]) + L".missing"};
    Application app({missing}, {Open, [](const FontChoice&) { return allowSave; },
        [](const EngineId&) { return allowSave; }, [](const Engine&) -> EngineId { return L"custom-test"; },
        [](Shortcut) { return allowSave; }, [](const std::wstring&) { return allowSave; },
        platform::SaveHistoryPreference, platform::RecordSearch, [](const std::vector<Engine>&) { return allowSave; },
        [](const std::optional<SelectionShortcut>&) { return allowSave; },
        [](const std::vector<EngineId>&) { return allowSave; },
        [](WindowBehavior) { return allowSave; }, [](bool) { return allowSave; },
        [](const std::optional<Shortcut>&) { return allowSave; }, [](const std::vector<std::wstring>&) { return allowSave; },
        [](bool) { return allowSave; }});
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    SearchWindow searchWindow;
    CHECK(InitializeSearchResources(app));
    CHECK(activeFont == FontChoice{});
    CHECK(app.preferences().font == missing);
    const HWND window = searchWindow.window = CreateSearchWindow(GetModuleHandleW(nullptr));
    CHECK(window && !IsWindowVisible(window) && edit && engineButton);
    CheckSearchPlaceholder(L"搜索网页…");
    CHECK(!drawingFactory && !write && !composition);
    CHECK(PrepareSearchUi(window));
    RECT windowClient{};
    GetClientRect(window, &windowClient);
    CHECK(windowClient.bottom == Scale(56));
    LOGFONTW inputMetrics{};
    CHECK(GetObjectW(editFont, sizeof(inputMetrics), &inputMetrics) && inputMetrics.lfHeight == -Scale(16));
    DWORD backdrop = 0;
    if (SUCCEEDED(DwmGetWindowAttribute(window, systemBackdropType, &backdrop, sizeof(backdrop)))) {
        CHECK(backdropEnabled && backdrop == 3); // DWMSBT_TRANSIENTWINDOW
    }
    CHECK(SUCCEEDED(Paint(window)));
    CHECK(engineBitmap && engineBitmap->GetPixelSize().width == static_cast<unsigned>(Scale(engineIconSize)));
    CHECK(chevronBitmaps[0] && chevronBitmaps[0]->GetPixelSize().width == static_cast<unsigned>(Scale(engineChevronSize)));
    for (const int monitorDpi : {96, 120, 144, 168, 192}) {
        CheckIconBlit(MulDiv(engineChevronSize, monitorDpi, USER_DEFAULT_SCREEN_DPI));
        CheckIconBlit(MulDiv(engineIconSize, monitorDpi, USER_DEFAULT_SCREEN_DPI));
    }
    CheckSvgRasterizer();
    for (unsigned index = 0; index < static_cast<unsigned>(TablerIcon::count); ++index) {
        constexpr unsigned size = 48;
        const auto pixels = TablerPixels(static_cast<TablerIcon>(index), RGB(0, 95, 184), size);
        CHECK(pixels && pixels->size() == size * size);
        CHECK(std::ranges::any_of(*pixels, [](auto pixel) { return pixel == 0xff005fb8; }));
        CHECK(std::ranges::any_of(*pixels, [](auto pixel) { return pixel == 0; }));
    }
    const auto browserIcon = TablerPixels(TablerIcon::browser, RGB(0, 95, 184), 48);
    CHECK((*browserIcon)[24 * 48 + 40] == 0xff005fb8);
    CHECK((*browserIcon)[24 * 48 + 45] == 0);
    for (const auto& engine : app.engines()) {
        for (const unsigned size : {16u, 24u, 32u, 48u}) {
            const auto pixels = EngineIcon(engine, size);
            CHECK(pixels && pixels->size() == size * size);
            CHECK(std::ranges::any_of(*pixels, [](auto pixel) { return pixel == 0xff000000; }));
            CHECK(std::ranges::any_of(*pixels, [](auto pixel) { return pixel == 0; }));
            CHECK(std::ranges::all_of(*pixels, [](auto pixel) { return (pixel & 0x00ffffff) == 0; }));
        }
    }
    const auto imported = ReadIconFile(argv[2]);
    CHECK(imported.size() == iconSize * iconSize);
    CHECK(!imported.front() && imported[iconSize * iconSize / 2 + iconSize / 2]);
    CHECK(ReadIconFile(missing.value.c_str()).empty());
    Engine svgEngine;
    svgEngine.svgIcon = ReadSvgIconFile(argv[3]);
    CHECK(!svgEngine.svgIcon.empty());
    CHECK(ReadSvgIconFile(argv[2]).empty());
    constexpr unsigned svgSize = 48;
    const auto svgPixels = EngineIcon(svgEngine, svgSize);
    CHECK(svgPixels && svgPixels->size() == svgSize * svgSize);
    CHECK((*svgPixels)[svgSize / 2] == 0);
    CHECK((*svgPixels)[svgSize * (svgSize / 2) + svgSize / 2] == 0); // The play triangle is transparent.
    CHECK((*svgPixels)[svgSize * (svgSize / 2) + svgSize / 4] == 0xff000000);
    CHECK(svgPixels == EngineIcon(*app.findEngine(L"youtube"), svgSize));

    for (const auto& test : searchCases) {
        CHECK(ApplySettingsEngine(app.engines()[test.engine].id));
        CHECK(SetWindowTextW(edit, test.query.data()));
        openedUrl.clear();
        SendMessageW(edit, WM_CHAR, VK_RETURN, 0);
        MSG posted{};
        CHECK(PeekMessageW(&posted, window, runSearch, runSearch, PM_REMOVE));
        DispatchMessageW(&posted);
        CHECK(openedUrl == test.expected);
    }
    SendMessageW(edit, WM_IME_STARTCOMPOSITION, 0, 0);
    SendMessageW(edit, WM_CHAR, VK_RETURN, 0);
    MSG posted{};
    CHECK(!PeekMessageW(&posted, window, runSearch, runSearch, PM_REMOVE));
    SendMessageW(edit, WM_IME_ENDCOMPOSITION, 0, 0);

    CHECK(HitClientPoint(window, {1, 1}) == HTCAPTION);
    CHECK(HitControl(window, edit) == HTCLIENT);
    CHECK(HitControl(window, engineButton) == HTCLIENT);

    LOGFONTW face{};
    const bool hasHarmonyFont = std::filesystem::is_regular_file(argv[1]);
    const FontChoice testFont = hasHarmonyFont ? FontChoice{FontSource::file, argv[1]}
                                               : FontChoice{FontSource::system, L"Tahoma"};
    CHECK(ApplySettingsFont(testFont, face));
    if (hasHarmonyFont) CheckHarmonyFont(face);
    CHECK(activeFont == testFont && app.preferences().font == testFont);
    CHECK(SUCCEEDED(Paint(window)));
    const HFONT previousFont = editFont;
    const auto previousFormat = engineFormat.Get();
    CHECK(!ApplySettingsFont(missing, face));
    CHECK(activeFont == testFont && app.preferences().font == testFont);
    CHECK(editFont == previousFont && engineFormat.Get() == previousFormat);

    const FontChoice system{FontSource::system, L"Segoe UI"};
    allowSave = false;
    CHECK(!ApplySettingsFont(system, face));
    CHECK(activeFont == testFont && app.preferences().font == testFont && editFont == previousFont);
    allowSave = true;
    CHECK(ApplySettingsFont(system, face));
    CHECK(activeFont == system && app.preferences().font == system);

    const HWND settings = CreateSettings(window, app, *drawingFactory.Get(), {activeFont, face},
        {ApplySettingsFont, ApplySettingsEngine, [](Shortcut shortcut) {
            return application->changeShortcut(shortcut) ? platform::ShortcutResult::applied : platform::ShortcutResult::saveFailed;
        }, nullptr, nullptr, nullptr, ApplySelectionShortcut});
    CHECK(settings && !IsWindowVisible(settings));
    const HWND nav = FindWindowExW(settings, nullptr, L"ListBox", nullptr);
    const HWND reset = FindWindowExW(settings, nullptr, L"Button", L"恢复默认");
    const HWND picker = FindWindowExW(settings, nav, L"ListBox", nullptr);
    CHECK(nav && reset && picker);
    CHECK(SendMessageW(nav, LB_GETCOUNT, 0, 0) == 7);
    CHECK(SendMessageW(nav, LB_SETCURSEL, 1, 0) == 1);
    Notify(settings, nav, LBN_SELCHANGE);
    const auto labels = FindWindowExW(settings, nullptr, L"Button", L"有字");
    const auto icons = FindWindowExW(settings, nullptr, L"Button", L"无字");
    CHECK(labels && icons && (GetWindowLongPtrW(icons, GWL_STYLE) & WS_VISIBLE));
    allowSave = false;
    Notify(settings, icons, BN_CLICKED);
    CHECK(app.preferences().showEngineTileNames);
    allowSave = true;
    Notify(settings, icons, BN_CLICKED);
    CHECK(!app.preferences().showEngineTileNames);
    Notify(settings, labels, BN_CLICKED);
    CHECK(app.preferences().showEngineTileNames);
    Notify(settings, icons, BN_CLICKED);
    CHECK(!app.preferences().showEngineTileNames);
    CHECK(SendMessageW(nav, LB_SETCURSEL, 0, 0) == 0);
    Notify(settings, nav, LBN_SELCHANGE);
    CHECK(FindWindowExW(settings, nullptr, L"Button", L"导入配置…"));
    CHECK(FindWindowExW(settings, nullptr, L"Button", L"导出引擎…"));
    CHECK(SendMessageW(nav, LB_GETCURSEL, 0, 0) == 0);
    const HWND startup = FindWindowExW(settings, nullptr, L"Button", L"登录 Windows 时启动 关");
    const HWND showOnLaunch = FindWindowExW(settings, nullptr, L"Button", L"手动启动时显示搜索框 开");
    const HWND closeToTray = FindWindowExW(settings, nullptr, L"Button", L"关闭搜索框后驻留托盘 开");
    CHECK(startup && showOnLaunch && closeToTray);
    allowSave = false;
    Notify(settings, startup, BN_CLICKED);
    Notify(settings, showOnLaunch, BN_CLICKED);
    CHECK(!app.preferences().startWithWindows && app.preferences().windowBehavior == WindowBehavior{});
    CHECK(FindWindowExW(settings, nullptr, L"STATIC", L"无法保存此设置，请重试。"));
    allowSave = true;
    Notify(settings, startup, BN_CLICKED);
    CHECK(app.preferences().startWithWindows);
    CHECK(FindWindowExW(settings, nullptr, L"Button", L"登录 Windows 时启动 开") == startup);
    Notify(settings, startup, BN_CLICKED);
    Notify(settings, showOnLaunch, BN_CLICKED);
    Notify(settings, closeToTray, BN_CLICKED);
    CHECK(!app.preferences().startWithWindows && app.preferences().windowBehavior == (WindowBehavior{false, false}));
    Notify(settings, showOnLaunch, BN_CLICKED);
    Notify(settings, closeToTray, BN_CLICKED);
    CHECK(app.preferences().windowBehavior == WindowBehavior{});

    CHECK(SendMessageW(nav, LB_SETCURSEL, 1, 0) == 1);
    Notify(settings, nav, LBN_SELCHANGE);
    CHECK(HitClientPoint(settings, {Scale(260), Scale(8)}) == HTCAPTION);
    CHECK(HitControl(settings, reset) == HTCLIENT);
    Notify(settings, reset, BN_CLICKED);
    CHECK(activeFont == FontChoice{} && app.preferences().font == FontChoice{});

    CHECK(SendMessageW(nav, LB_SETCURSEL, 2, 0) == 2);
    Notify(settings, nav, LBN_SELCHANGE);
    CHECK(HitControl(settings, picker) == HTCLIENT);
    CHECK(SendMessageW(picker, LB_SETCURSEL, 2, 0) == 2);
    Notify(settings, picker, LBN_SELCHANGE);
    CHECK(app.preferences().engine == L"bing");
    const HWND setDefault = FindWindowExW(settings, nullptr, L"Button", L"设为默认");
    CHECK(setDefault);
    Notify(settings, setDefault, BN_CLICKED);
    CHECK(app.preferences().engine == L"google");
    wchar_t label[64]{};
    GetWindowTextW(engineButton, label, static_cast<int>(std::size(label)));
    CHECK(std::wstring_view(label) == L"搜索引擎：Google");
    CHECK(ApplySettingsEngine(L"baidu"));
    CHECK(SendMessageW(picker, LB_GETCURSEL, 0, 0) == 1);
    allowSave = false;
    CHECK(SendMessageW(picker, LB_SETCURSEL, 2, 0) == 2);
    Notify(settings, picker, LBN_SELCHANGE);
    Notify(settings, setDefault, BN_CLICKED);
    CHECK(app.preferences().engine == L"baidu");
    CHECK(SendMessageW(picker, LB_GETCURSEL, 0, 0) == 2);
    allowSave = true;

    const HWND add = FindWindowExW(settings, nullptr, L"Button", L"添加搜索引擎");
    const HWND save = FindWindowExW(settings, nullptr, L"Button", L"添加");
    const HWND name = FindWindowExW(settings, nullptr, L"Edit", nullptr);
    const HWND url = FindWindowExW(settings, name, L"Edit", nullptr);
    CHECK(add && save && name && url);
    Notify(settings, add, BN_CLICKED);
    CHECK(HitControl(settings, name) == HTCLIENT);
    CHECK(SetWindowTextW(name, L"测试引擎"));
    CHECK(SetWindowTextW(url, L"https://example.org"));
    Notify(settings, save, BN_CLICKED);
    CHECK(GetWindowLongPtrW(name, GWL_STYLE) & WS_VISIBLE);
    CHECK(app.engines().size() == 11);
    CHECK(SetWindowTextW(url, L"https://example.org/find/%s"));
    Notify(settings, save, BN_CLICKED);
    CHECK(!(GetWindowLongPtrW(name, GWL_STYLE) & WS_VISIBLE));
    CHECK(app.engines().back().id == L"custom-test");
    CHECK(SendMessageW(picker, LB_GETCOUNT, 0, 0) == 12);
    CHECK(SendMessageW(picker, LB_SETCURSEL, 11, 0) == 11);
    Notify(settings, picker, LBN_SELCHANGE);
    Notify(settings, setDefault, BN_CLICKED);
    CHECK(app.activeEngine().id == L"custom-test");
    CHECK(SUCCEEDED(Paint(window)) && bitmapEngine == L"custom-test");
    const auto oldIcon = engineBitmapPixels;
    auto editedEngine = app.activeEngine();
    editedEngine.svgIcon = svgEngine.svgIcon;
    CHECK(app.editEngine(editedEngine) == AddEngineResult::added);
    CHECK(SUCCEEDED(Paint(window)) && engineBitmap && bitmapEngine == L"custom-test");
    CHECK(engineBitmapPixels && oldIcon && *engineBitmapPixels != *oldIcon);
    CHECK(engineBitmapPixels == EngineIcon(editedEngine, Scale(engineIconSize)));
    const UINT originalDpi = dpi;
    RECT initialBounds{};
    CHECK(GetWindowRect(window, &initialBounds));
    for (const UINT monitorDpi : {96u, 120u, 144u, 168u, 192u, originalDpi}) {
        SendMessageW(window, WM_DPICHANGED, MAKEWPARAM(monitorDpi, monitorDpi),
            reinterpret_cast<LPARAM>(&initialBounds));
        CHECK(SUCCEEDED(Paint(window)) && engineBitmap && chevronBitmaps[0]);
        CHECK(engineBitmap->GetPixelSize().width == static_cast<unsigned>(Scale(engineIconSize)));
        CHECK(chevronBitmaps[0]->GetPixelSize().width == static_cast<unsigned>(Scale(engineChevronSize)));
    }
    CHECK(app.editEngine(Engine{editedEngine.id, editedEngine.name, editedEngine.url,
        editedEngine.group, editedEngine.color, {}, editedEngine.alias}) == AddEngineResult::added);
    CHECK(SetWindowTextW(edit, L" C++ 中文 "));
    SendMessageW(edit, WM_CHAR, VK_RETURN, 0);
    CHECK(PeekMessageW(&posted, window, runSearch, runSearch, PM_REMOVE));
    DispatchMessageW(&posted);
    CHECK(openedUrl == L"https://example.org/find/C%2B%2B%20%E4%B8%AD%E6%96%87");

    CHECK(SetWindowTextW(edit, L"#gh"));
    CHECK(displayedEngine == L"github");
    CHECK(SetWindowTextW(edit, L"#gh Direct2D"));
    Search(window);
    CHECK(openedUrl == L"https://github.com/search?q=Direct2D");
    CHECK(displayedEngine == L"custom-test" && app.activeEngine().id == L"custom-test");

    CHECK(SendMessageW(nav, LB_SETCURSEL, 3, 0) == 3);
    Notify(settings, nav, LBN_SELCHANGE);
    const HWND recorder = FindWindowExW(settings, nullptr, L"Button", L"更改快捷键");
    CHECK(recorder);
    Notify(settings, recorder, BN_CLICKED);
    MSG key{};
    key.hwnd = recorder;
    key.wParam = VK_MENU;
    key.message = WM_SYSKEYDOWN;
    key.time = 10;
    CHECK(HandleSettingsMessage(key));
    key.message = WM_SYSKEYUP;
    key.time = 60;
    CHECK(HandleSettingsMessage(key));
    key.message = WM_SYSKEYDOWN;
    key.time = 90;
    CHECK(HandleSettingsMessage(key));
    key.message = WM_SYSKEYUP;
    key.time = 140;
    CHECK(HandleSettingsMessage(key));
    const HWND shortcutSave = FindWindowExW(settings, nullptr, L"Button", L"保存");
    CHECK(shortcutSave && IsWindowEnabled(shortcutSave));
    Notify(settings, shortcutSave, BN_CLICKED);
    CHECK(app.preferences().shortcut == Shortcut{});

    {
        CHECK(FindWindowExW(settings, nullptr, L"Button", L"应用"));
        CHECK(FindWindowExW(settings, nullptr, L"Button", L"录制剪贴板快捷键"));
        CHECK(app.changeExcludedApps({L"mstsc.exe", L"游戏.exe"}));
        CHECK(IsDoubleAltExcluded(L"C:\\Windows\\System32\\MSTSC.EXE", app.preferences().doubleAltExcludedApps));
        CHECK(!IsDoubleAltExcluded(L"C:\\Windows\\other.exe", app.preferences().doubleAltExcludedApps));
        CHECK(!app.changeExcludedApps({L"C:\\Windows\\mstsc.exe"}));
        platform::ShortcutRegistration clipboard(window, 0, 5);
        clipboardRegistration = &clipboard;
        const Shortcut binding{shortcutControl | shortcutShift, VK_F23};
        CHECK(ApplyClipboardShortcut(binding) == platform::ShortcutResult::applied);
        CHECK(clipboard.receives(5) && app.preferences().clipboardShortcut == binding);
        SearchClipboard(window, NormalizeSearchText(L"\r\n #gh\tDirect2D "));
        CHECK(openedUrl == L"https://example.org/find/%23gh%20Direct2D");
        CHECK(ApplyClipboardShortcut(std::nullopt) == platform::ShortcutResult::applied);
        CHECK(!clipboardShortcutActive && !app.preferences().clipboardShortcut);
        clipboardRegistration = nullptr;
    }

    {
        platform::ShortcutRegistration selection(window, 0, 3);
        selectionRegistration = &selection;
        const HWND engineChoice = FindWindowExW(settings, nullptr, L"ComboBox", nullptr);
        const HWND modeChoice = FindWindowExW(settings, engineChoice, L"ComboBox", nullptr);
        const HWND selectionRecorder = FindWindowExW(settings, nullptr, L"Button", L"录制划词快捷键");
        const HWND clearSelection = FindWindowExW(settings, nullptr, L"Button", L"清除");
        CHECK(engineChoice && modeChoice && selectionRecorder && clearSelection && !IsWindowEnabled(clearSelection));
        CHECK(SendMessageW(modeChoice, CB_GETCURSEL, 0, 0) == 1);
        CHECK(SendMessageW(modeChoice, CB_SETCURSEL, 0, 0) == 0);
        const auto github = SendMessageW(engineChoice, CB_FINDSTRINGEXACT, static_cast<WPARAM>(-1),
            reinterpret_cast<LPARAM>(L"GitHub  #gh"));
        CHECK(github != CB_ERR && SendMessageW(engineChoice, CB_SETCURSEL, github, 0) == github);
        Notify(settings, selectionRecorder, BN_CLICKED);
        BYTE keyboard[256]{};
        CHECK(GetKeyboardState(keyboard));
        BYTE modifiers[256]{};
        modifiers[VK_CONTROL] = modifiers[VK_SHIFT] = 0x80;
        CHECK(SetKeyboardState(modifiers));
        MSG combination{selectionRecorder, WM_KEYDOWN, VK_F24};
        CHECK(HandleSettingsMessage(combination));
        CHECK(SetKeyboardState(keyboard));
        CHECK(IsWindowEnabled(shortcutSave));

        allowSave = false;
        Notify(settings, shortcutSave, BN_CLICKED);
        CHECK(!selectionShortcutActive && !app.preferences().selectionShortcut);
        allowSave = true;
        Notify(settings, shortcutSave, BN_CLICKED);
        const SelectionShortcut binding{{shortcutControl | shortcutShift, VK_F24}, L"github"};
        CHECK(selectionShortcutActive && app.preferences().selectionShortcut == binding);
        CHECK(app.activeEngine().id == L"custom-test");

        const auto previousUrl = openedUrl;
        FinishSelection(window, L"   ", nullptr, binding.engine);
        CHECK(openedUrl == previousUrl && !browserError);
        FinishSelection(window, L"#gh Direct2D", nullptr, binding.engine);
        CHECK(openedUrl == L"https://github.com/search?q=%23gh%20Direct2D");
        allowOpen = false;
        FinishSelection(window, L"Direct2D", nullptr, binding.engine);
        CHECK(browserError && displayedEngine == binding.engine && ReadSearchText() == L"Direct2D");
        allowOpen = true;
        Search(window);
        CHECK(!browserError && openedUrl == L"https://github.com/search?q=Direct2D");

        CHECK(SendMessageW(modeChoice, CB_SETCURSEL, 1, 0) == 1);
        Notify(settings, modeChoice, CBN_SELENDOK);
        CHECK(app.preferences().selectionShortcut->preview && selection.receives(3));
        openedUrl.clear();
        ShowWindow(window, SW_HIDE);
        CHECK(SetWindowTextW(edit, L"previous query"));
        FinishSelection(window, L"   ", nullptr, binding.engine, true);
        CHECK(IsWindowVisible(window) && openedUrl.empty() && ReadSearchText().empty() && displayedEngine == binding.engine);
        FinishSelection(window, L"\r\n #gh\tDirect2D\r\nguide  ", nullptr, binding.engine, true);
        CHECK(openedUrl.empty() && ReadSearchText() == L"#gh Direct2D guide" && displayedEngine == L"github");
        CHECK(previewInput && literalInput);
        CHECK(SetWindowTextW(edit, L"#gh edited"));
        Search(window);
        CHECK(openedUrl == L"https://github.com/search?q=%23gh%20edited" && !literalInput && !previewInput);
        ShowWindow(window, SW_HIDE);

        const auto custom = SendMessageW(engineChoice, CB_GETCOUNT, 0, 0) - 1;
        CHECK(SendMessageW(engineChoice, CB_SETCURSEL, custom, 0) == custom);
        Notify(settings, engineChoice, CBN_SELENDOK);
        CHECK(app.preferences().selectionShortcut == (SelectionShortcut{binding.shortcut, L"custom-test", true}));
        CHECK(selection.receives(3));
        Notify(settings, clearSelection, BN_CLICKED);
        CHECK(!selectionShortcutActive && !selection.receives(3) && !app.preferences().selectionShortcut);
        selectionRegistration = nullptr;
    }

    CHECK(SendMessageW(nav, LB_SETCURSEL, 4, 0) == 4);
    Notify(settings, nav, LBN_SELCHANGE);
    const HWND selector = FindWindowExW(settings, nullptr, L"Button", L"跟随 Windows 默认浏览器");
    const HWND other = FindWindowExW(settings, nullptr, L"Button", L"选择其他浏览器…");
    const HWND browserPath = FindWindowExW(settings, url, L"Edit", nullptr);
    CHECK(selector && other && browserPath);
    Notify(settings, selector, BN_CLICKED);
    const HWND browsers = FindWindowExW(settings, picker, L"ListBox", nullptr);
    CHECK(browsers && SendMessageW(browsers, LB_GETCOUNT, 0, 0) >= 1);
    Notify(settings, other, BN_CLICKED);
    SetWindowTextW(browserPath, L"missing.exe");
    key.hwnd = browserPath;
    key.wParam = VK_RETURN;
    key.message = WM_KEYDOWN;
    CHECK(HandleSettingsMessage(key));
    CHECK(app.preferences().browser.empty());
    wchar_t executable[32768]{};
    CHECK(GetModuleFileNameW(nullptr, executable, static_cast<DWORD>(std::size(executable))));
    SetWindowTextW(browserPath, executable);
    CHECK(HandleSettingsMessage(key));
    CHECK(app.preferences().browser == executable);
    FinishSelection(window, L"划词测试");
    CHECK(openedBrowser == executable);
    CHECK(openedUrl == L"https://example.org/find/%E5%88%92%E8%AF%8D%E6%B5%8B%E8%AF%95");
    allowOpen = false;
    CHECK(SetWindowTextW(edit, L"test"));
    Search(window);
    CHECK(browserError);
    CHECK(SetWindowTextW(edit, L"retry"));
    CHECK(!browserError);
    allowOpen = true;

    {
        TestRegistry registry;
        registry.redirect();
        CHECK(SendMessageW(nav, LB_SETCURSEL, 5, 0) == 5);
        Notify(settings, nav, LBN_SELCHANGE);
        const HWND historyToggle = FindWindowExW(settings, nullptr, L"Button", L"记录历史　关");
        const HWND history = FindWindowExW(settings, browsers, L"ListBox", nullptr);
        const HWND aliasField = FindWindowExW(settings, browserPath, L"Edit", nullptr);
        const HWND queryFilter = FindWindowExW(settings, aliasField, L"Edit", nullptr);
        const HWND appFilter = FindWindowExW(settings, queryFilter, L"Edit", nullptr);
        CHECK(historyToggle && history && aliasField && queryFilter && appFilter);
        Notify(settings, historyToggle, BN_CLICKED);
        CHECK(app.preferences().recordHistory);
        FinishSelection(window, L"#gh 是原文");
        CHECK(openedUrl == L"https://example.org/find/%23gh%20%E6%98%AF%E5%8E%9F%E6%96%87");
        Notify(settings, nav, LBN_SELCHANGE);
        CHECK(SendMessageW(history, LB_GETCOUNT, 0, 0) == 1);
        CHECK(SetWindowTextW(queryFilter, L"#GH"));
        SendMessageW(settings, WM_TIMER, 1, 0);
        CHECK(SendMessageW(history, LB_GETCOUNT, 0, 0) == 1);
        CHECK(SetWindowTextW(appFilter, L"VS Code"));
        SendMessageW(settings, WM_TIMER, 1, 0);
        CHECK(SendMessageW(history, LB_GETCOUNT, 0, 0) == 0);
        CHECK(SetWindowTextW(appFilter, L""));
        SendMessageW(settings, WM_TIMER, 1, 0);
        CHECK(SendMessageW(history, LB_GETCOUNT, 0, 0) == 1);
        CHECK(SendMessageW(history, LB_SETCURSEL, 0, 0) == 0);
        key.hwnd = history;
        key.wParam = VK_RETURN;
        CHECK(HandleSettingsMessage(key));
        CHECK(SendMessageW(history, LB_GETCOUNT, 0, 0) == 2);
        CHECK(SendMessageW(history, LB_SETCURSEL, 0, 0) == 0);
        key.wParam = VK_DELETE;
        CHECK(HandleSettingsMessage(key));
        CHECK(SendMessageW(history, LB_GETCOUNT, 0, 0) == 1);
        Notify(settings, historyToggle, BN_CLICKED);
        CHECK(!app.preferences().recordHistory && platform::LoadHistory().size() == 1);
    }

    CHECK(SendMessageW(nav, LB_SETCURSEL, 2, 0) == 2);
    Notify(settings, nav, LBN_SELCHANGE);
    const HWND pin = FindWindowExW(settings, nullptr, L"Button", L"置顶");
    CHECK(pin && SendMessageW(picker, LB_SETCURSEL, 0, 0) == 0);
    Notify(settings, picker, LBN_SELCHANGE);
    Notify(settings, pin, BN_CLICKED);
    CHECK(app.isPinned(L"bing"));
    CHECK(FindWindowExW(settings, nullptr, L"Button", L"取消置顶") == pin);
    Notify(settings, pin, BN_CLICKED);
    CHECK(!app.isPinned(L"bing"));

    MSG escape{};
    escape.hwnd = picker;
    escape.message = WM_KEYDOWN;
    escape.wParam = VK_ESCAPE;
    CHECK(HandleSettingsMessage(escape) && !IsWindow(settings));

    const EngineFlyoutActions actions{SelectFlyoutEngine, OpenFlyoutSettings, OnFlyoutClosed};
    CHECK(app.togglePinnedEngine(L"github"));
    engineFlyout = CreateEngineFlyout(engineButton, app, *drawingFactory.Get(), *write.Get(), *engineFormat.Get(), actions);
    CHECK(engineFlyout);
    const HWND pinnedTile = FindWindowExW(engineFlyout, nullptr, L"Button", L"GitHub");
    CHECK(pinnedTile && pinnedTile == FindWindowExW(engineFlyout, nullptr, L"Button", nullptr));
    DestroyWindow(engineFlyout);
    CHECK(app.togglePinnedEngine(L"github"));

    CHECK(SetWindowTextW(edit, L"#g"));
    const auto suggestions = app.suggestEngines(ReadSearchText());
    suggestionFlyout = CreateEngineFlyout(edit, app, *drawingFactory.Get(), *write.Get(), *engineFormat.Get(),
        {AcceptEngineSuggestion, nullptr, OnSuggestionClosed}, displayedEngine, &suggestions);
    CHECK(suggestionFlyout && (GetWindowLongPtrW(suggestionFlyout, GWL_EXSTYLE) & WS_EX_NOACTIVATE));
    const HWND suggestionRows = FindWindowExW(suggestionFlyout, nullptr, L"ListBox", nullptr);
    CHECK(SendMessageW(suggestionRows, LB_GETCOUNT, 0, 0) > 1);
    CHECK(HandleEngineSuggestionKey(suggestionFlyout, VK_DOWN) && SendMessageW(suggestionRows, LB_GETCURSEL, 0, 0) == 1);
    CHECK(HandleEngineSuggestionKey(suggestionFlyout, VK_UP) && SendMessageW(suggestionRows, LB_GETCURSEL, 0, 0) == 0);
    CHECK(RefreshEngineSuggestions(suggestionFlyout, app.suggestEngines(L"#gh")));
    CHECK(SendMessageW(suggestionRows, LB_GETCOUNT, 0, 0) == 1);
    CHECK(HandleEngineSuggestionKey(suggestionFlyout, VK_TAB));
    CHECK(!suggestionFlyout && ReadSearchText().empty() && displayedEngine == L"github");
    CHECK(SetWindowTextW(edit, L"Direct2D"));
    Search(window);
    CHECK(openedUrl == L"https://github.com/search?q=Direct2D" && displayedEngine == L"custom-test");

    engineFlyout = CreateEngineFlyout(engineButton, app, *drawingFactory.Get(), *write.Get(), *engineFormat.Get(), actions);
    CHECK(engineFlyout && !IsWindowVisible(engineFlyout));
    CHECK(SUCCEEDED(Paint(window)) && chevronBitmaps[1]);
    const auto colorTile = FindWindowExW(engineFlyout, nullptr, L"Button", app.activeEngine().name.c_str());
    const auto githubTile = FindWindowExW(engineFlyout, nullptr, L"Button", L"GitHub");
    const auto stackTile = FindWindowExW(engineFlyout, nullptr, L"Button", L"Stack Overflow");
    CHECK(colorTile && githubTile && stackTile);
    CHECK(!FindWindowExW(engineFlyout, nullptr, L"ListBox", nullptr));
    CheckFlyoutPaint(engineFlyout, colorTile, githubTile);
    RECT githubBounds{}, stackBounds{};
    CHECK(GetWindowRect(githubTile, &githubBounds) && GetWindowRect(stackTile, &stackBounds));
    CHECK(githubBounds.top == stackBounds.top && githubBounds.right < stackBounds.left);
    CHECK(githubBounds.right - githubBounds.left == githubBounds.bottom - githubBounds.top);
    HWND tooltip = nullptr;
    while ((tooltip = FindWindowExW(nullptr, tooltip, TOOLTIPS_CLASSW, nullptr))) {
        if (GetWindow(tooltip, GW_OWNER) == engineFlyout) break;
    }
    CHECK(tooltip && SendMessageW(tooltip, TTM_GETTOOLCOUNT, 0, 0) == static_cast<LRESULT>(app.engines().size() + 1));

    MSG flyoutKey{};
    flyoutKey.hwnd = githubTile;
    flyoutKey.message = WM_KEYDOWN;
    flyoutKey.wParam = VK_RIGHT;
    CHECK(HandleEngineFlyoutMessage(engineFlyout, flyoutKey) && GetFocus() == stackTile);
    flyoutKey.hwnd = stackTile;
    flyoutKey.wParam = VK_RETURN;
    CHECK(HandleEngineFlyoutMessage(engineFlyout, flyoutKey));
    CHECK(!engineFlyout && displayedEngine == L"stackoverflow" && app.activeEngine().id == L"custom-test");

    engineFlyout = CreateEngineFlyout(engineButton, app, *drawingFactory.Get(), *write.Get(), *engineFormat.Get(), actions, displayedEngine);
    CHECK(engineFlyout);
    escape.hwnd = engineFlyout;
    CHECK(HandleEngineFlyoutMessage(engineFlyout, escape));
    CHECK(!engineFlyout && displayedEngine == L"stackoverflow" && app.activeEngine().id == L"custom-test");

    engineFlyout = CreateEngineFlyout(engineButton, app, *drawingFactory.Get(), *write.Get(), *engineFormat.Get(), actions, displayedEngine);
    CHECK(engineFlyout);
    const auto clickTile = FindWindowExW(engineFlyout, nullptr, L"Button", L"GitHub");
    CHECK(clickTile);
    Notify(engineFlyout, clickTile, BN_CLICKED);
    CHECK(!engineFlyout && displayedEngine == L"github" && app.activeEngine().id == L"custom-test");

    std::vector<Engine> extraEngines;
    for (int index = 0; index < 12; ++index) {
        const auto suffix = std::to_wstring(index);
        extraEngines.push_back({L"custom-" + suffix, L"Engine " + suffix, L"https://example.com/?q=%s"});
    }
    const Application many({}, {}, std::move(extraEngines));
    engineFlyout = CreateEngineFlyout(engineButton, many, *drawingFactory.Get(), *write.Get(), *engineFormat.Get(), actions);
    CHECK(engineFlyout);
    const auto scrollbar = FindWindowExW(engineFlyout, nullptr, L"ScrollBar", nullptr);
    const auto addTile = FindWindowExW(engineFlyout, nullptr, L"Button", L"添加搜索引擎");
    CHECK(scrollbar && addTile && !(GetWindowLongPtrW(addTile, GWL_STYLE) & WS_VISIBLE));
    SendMessageW(engineFlyout, WM_VSCROLL, SB_BOTTOM, reinterpret_cast<LPARAM>(scrollbar));
    CHECK(GetWindowLongPtrW(addTile, GWL_STYLE) & WS_VISIBLE);
    SendMessageW(engineFlyout, WM_VSCROLL, SB_TOP, reinterpret_cast<LPARAM>(scrollbar));
    CHECK(!(GetWindowLongPtrW(addTile, GWL_STYLE) & WS_VISIBLE));
    flyoutKey.hwnd = engineFlyout;
    flyoutKey.wParam = VK_END;
    CHECK(HandleEngineFlyoutMessage(engineFlyout, flyoutKey));
    CHECK(GetWindowLongPtrW(addTile, GWL_STYLE) & WS_VISIBLE);
    Notify(engineFlyout, addTile, BN_CLICKED);
    CHECK(!engineFlyout && HasOpenSettings());
    const auto addSettings = FindOwnedSettings(window);
    CHECK(addSettings && IsWindowVisible(FindWindowExW(addSettings, nullptr, L"Static", L"名称")));
    SendMessageW(addSettings, WM_CLOSE, 0, 0);
    CHECK(ApplySettingsFont(testFont, face));
    const HFONT cachedFont = editFont;
    const auto cachedIcon = EngineIcon(*app.findEngine(L"youtube"), svgSize);
    ShowWindow(window, SW_HIDE);
    CHECK(SetWindowTextW(edit, L"retained input"));
    const HWND settingsWithHiddenSearch = CreateSettings(window, app, *drawingFactory.Get(), {activeFont, face}, {});
    CHECK(settingsWithHiddenSearch && HasOpenSettings());
    SendMessageW(window, WM_TIMER, graphicsIdleTimer, 0);
    CHECK(composition && drawingDevice);
    DestroyWindow(settingsWithHiddenSearch);
    SendMessageW(window, WM_TIMER, graphicsIdleTimer, 0);
    CHECK(!composition && !drawingDevice && !dxgiDevice && !surface && !caretVisual);
    CHECK(editFont == cachedFont);
    CHECK(cachedIcon && EngineIcon(*app.findEngine(L"youtube"), svgSize) == cachedIcon);
    CHECK(app.preferences().font == testFont && ReadSearchText() == L"retained input");
    OpenSettings(window);
    CHECK(HasOpenSettings() && drawingDevice && !IsWindowVisible(window));
    CHECK(!composition && !compositionTarget && !caretVisual);
    if (hasHarmonyFont) CheckHarmonyFont(face);
    const HWND reopenedSettings = FindOwnedSettings(window);
    CHECK(reopenedSettings);
    const auto* settingsDevice = drawingDevice.Get();
    RevealSearch(window);
    CHECK(IsWindowVisible(window) && composition && drawingDevice.Get() == settingsDevice);
    SendMessageW(reopenedSettings, WM_CLOSE, 0, 0);
    CHECK(!HasOpenSettings());
    ShowWindow(window, SW_HIDE);
    SendMessageW(window, WM_TIMER, graphicsIdleTimer, 0);
    CHECK(!composition && !drawingDevice);
    RevealSearch(window);
    CHECK(IsWindowVisible(window) && composition && ReadSearchText() == L"retained input");
    if (hasHarmonyFont) CheckHarmonyFont(face);
    CHECK(SUCCEEDED(Paint(window)) && engineBitmap);
    CHECK(SetWindowTextW(edit, L"https://example.org/?q=a=b"));
    CHECK(linkAvailable && WindowHeight() > height);
    MSG command{edit, WM_KEYDOWN, VK_RETURN};
    CHECK(HandleSearchCommand(window, command, shortcutControl));
    CHECK(openedUrl == L"https://example.org/?q=a=b" && !linkAvailable && ReadSearchText().empty());
    CHECK(SetWindowTextW(edit, L"repeat me"));
    Search(window);
    command.wParam = VK_UP;
    CHECK(HandleSearchCommand(window, command, 0) && ReadSearchText() == L"repeat me");
    CHECK(app.togglePinnedEngine(L"github"));
    command.wParam = L'1';
    CHECK(HandleSearchCommand(window, command, shortcutAlt));
    CHECK(displayedEngine == L"github" && ReadSearchText() == L"repeat me" && app.activeEngine().id == L"custom-test");
    Search(window);
    CHECK(openedUrl == L"https://github.com/search?q=repeat%20me");
    shortcutActive = true;
    SendMessageW(window, WM_CLOSE, 0, 0);
    CHECK(IsWindow(window) && !IsWindowVisible(window));
    CHECK(app.changeWindowBehavior({true, false}));
    SendMessageW(window, WM_CLOSE, 0, 0);
    CHECK(!IsWindow(window));
    searchWindow.window = nullptr;

    i18n::SetLanguage(Language::english);
    searchWindow.window = CreateSearchWindow(GetModuleHandleW(nullptr));
    CHECK(searchWindow.window && edit);
    CheckSearchPlaceholder(L"Search the web…");
    return 0;
} catch (const std::exception& error) {
    std::cerr << error.what() << '\n';
    return 1;
}
