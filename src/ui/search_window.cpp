#include "../platform/localization.h"
#include <windows.h>
#include <windowsx.h>
#include <commctrl.h>
#include <d2d1_1.h>
#include <d3d11.h>
#include <dxgi1_3.h>
#include <dcomp.h>
#include <dwrite_3.h>
#include <dwmapi.h>
#include <imm.h>
#include <shellapi.h>
#include <wrl/client.h>

#include "search_window.h"
#include "engine_flyout.h"
#include "engine_icons.h"
#include "settings.h"
#include "visual_style.h"
#include "../platform/performance.h"
#include "../platform/selection.h"
#include "../platform/windows_services.h"
#include "../platform/browser.h"

#include <algorithm>
#include <array>
#include <future>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>
#include <thread>

#pragma comment(linker, "/manifestdependency:\"type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'\"")

using Microsoft::WRL::ComPtr;
namespace yisou::ui {
using i18n::Text;
using i18n::Tr;

namespace {
constexpr int width = 580;
constexpr int height = 56;
constexpr int fallbackColor = 0xfcfdff;
constexpr int textColor = 0x1a2029;
constexpr int mutedColor = 0x536273;
constexpr int accentColor = 0x005fb8;
constexpr UINT engineButtonId = 100;
constexpr UINT runSearch = WM_APP + 1;
constexpr UINT shortcutTriggered = WM_APP + 2;
constexpr UINT trayMessage = WM_APP + 3;
constexpr UINT openExisting = WM_APP + 4;
constexpr UINT restartConfiguration = WM_APP + 5;
constexpr wchar_t searchClassName[] = L"YiSouResidentSearch";
constexpr UINT_PTR selectionTimer = 2;
constexpr UINT_PTR graphicsIdleTimer = 3;
constexpr UINT graphicsIdleDelay = 1500;
constexpr UINT trayId = 1;
constexpr UINT_PTR caretTimer = 1;
constexpr int inputFontSize = 16;
constexpr int editLeft = 20;
constexpr int editWidth = 397;
constexpr int editHeight = 30;
constexpr int caretHeight = 21;
constexpr int engineWidth = 134;
constexpr int engineLeft = width - engineWidth - 12;
constexpr int engineHeight = 34;
constexpr int engineTop = (height - engineHeight) / 2;
constexpr int engineIconSize = 20;
constexpr int engineChevronSize = 16;
constexpr DWORD privateFontFlags = FR_PRIVATE | FR_NOT_ENUM;

// These public Windows 11 22H2 constants also support builds with the 22000 SDK.
constexpr DWORD systemBackdropType = 38; // DWMWA_SYSTEMBACKDROP_TYPE
constexpr DWORD desktopAcrylic = 3;    // DWMSBT_TRANSIENTWINDOW

Application* application = nullptr;
platform::ShortcutRegistration* shortcutRegistration = nullptr;
platform::ShortcutRegistration* selectionRegistration = nullptr;
platform::ShortcutRegistration* clipboardRegistration = nullptr;
bool shortcutActive = false;
bool selectionShortcutActive = false;
bool clipboardShortcutActive = false;
bool browserError = false;
bool linkAvailable = false;
bool trayActive = false;
UINT taskbarCreated = 0;
HWND selectionSource = nullptr;
EngineId selectionEngine;
bool selectionPreview = false;
ULONGLONG selectionStarted = 0;
ULONGLONG selectionDeadline = 0;
std::stop_source selectionStop{std::nostopstate};
std::future<std::wstring> selectedText;
bool uninitializeCom = false;

HWND edit = nullptr;
HWND engineButton = nullptr;
HWND engineFlyout = nullptr;
HWND suggestionFlyout = nullptr;
EngineId temporaryEngine;
EngineId displayedEngine;
std::wstring displayedEngineName;
bool literalInput = false;
SearchOrigin literalOrigin = SearchOrigin::selection;
std::wstring lastQuery;
bool previewInput = false;
std::wstring retrySource;
HFONT editFont = nullptr;
LOGFONTW uiFontTemplate{};
FontChoice activeFont;
UINT dpi = USER_DEFAULT_SCREEN_DPI;
int editTopPixels = 0;
bool composing = false;
std::wstring compositionText;
size_t compositionCursor = 0;
std::optional<int> compositionCaretX;
bool backdropEnabled = false;
bool engineHovered = false;
bool enginePressed = false;
bool caretVisible = false;
ComPtr<ID2D1Factory1> drawingFactory;
ComPtr<IDWriteFactory3> write;
ComPtr<IDCompositionDesktopDevice> composition;
ComPtr<ID2D1Device> drawingDevice;
ComPtr<IDXGIDevice3> dxgiDevice;
ComPtr<IDCompositionTarget> compositionTarget;
ComPtr<IDCompositionVisual2> visual;
ComPtr<IDCompositionVisual3> caretVisual;
ComPtr<IDCompositionSurface> surface;
SIZE surfaceSize{};
ComPtr<IDWriteTextFormat> engineFormat;
ComPtr<ID2D1Bitmap> engineBitmap;
std::array<ComPtr<ID2D1Bitmap>, 2> chevronBitmaps;
EngineId bitmapEngine;

int Scale(int value) { return MulDiv(value, dpi, USER_DEFAULT_SCREEN_DPI); }
int WindowHeight() { return height + ((browserError || linkAvailable) ? 27 : 0); }

struct MonitorInfo : MONITORINFOEXW {
    MonitorInfo() : MONITORINFOEXW{} { cbSize = sizeof(MONITORINFOEXW); }
};

bool ReadMonitor(HMONITOR monitor, MonitorInfo& info)
{
    return GetMonitorInfoW(monitor, reinterpret_cast<MONITORINFO*>(&info)) != FALSE;
}

HMONITOR TargetMonitor(HWND foreground)
{
    if (foreground) return MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST);
    POINT cursor{};
    GetCursorPos(&cursor);
    return MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY);
}

void PositionSearch(HWND window, HMONITOR monitor)
{
    MonitorInfo info;
    if (!ReadMonitor(monitor, info)) return;
    const auto& area = info.rcWork;
    const auto saved = platform::LoadDisplayPosition(info.szDevice);
    const auto reposition = [&] {
        const LONG roomX = std::max(0L, area.right - area.left - Scale(width));
        const LONG roomY = std::max(0L, area.bottom - area.top - Scale(WindowHeight()));
        const LONG x = saved ? std::clamp<LONG>(Scale(saved->x), 0, roomX) : roomX / 2;
        const LONG y = saved ? std::clamp<LONG>(Scale(saved->y), 0, roomY) : roomY / 3;
        SetWindowPos(window, nullptr, area.left + x, area.top + y, 0, 0, SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE);
    };
    const auto previousDpi = dpi;
    reposition();
    if (previousDpi != dpi) reposition();
}

HRESULT CreateFileFontCollection(const wchar_t* path, IDWriteFontCollection1** collection)
{
    ComPtr<IDWriteFontFaceReference> face;
    ComPtr<IDWriteFontSetBuilder> builder;
    ComPtr<IDWriteFontSet> fontSet;
    HRESULT result = write->CreateFontFaceReference(path, nullptr, 0, DWRITE_FONT_SIMULATIONS_NONE, face.GetAddressOf());
    if (SUCCEEDED(result)) result = write->CreateFontSetBuilder(builder.GetAddressOf());
    if (SUCCEEDED(result)) result = builder->AddFontFaceReference(face.Get());
    if (SUCCEEDED(result)) result = builder->CreateFontSet(fontSet.GetAddressOf());
    return SUCCEEDED(result) ? write->CreateFontCollectionFromFontSet(fontSet.Get(), collection) : result;
}

bool ResolveFont(const FontChoice& choice, LOGFONTW& face, IDWriteTextFormat** format)
{
    ComPtr<IDWriteFontCollection1> collection;
    UINT32 familyIndex = 0;
    if (choice.source == FontSource::file) {
        if (FAILED(CreateFileFontCollection(choice.value.c_str(), collection.GetAddressOf()))) return false;
    } else {
        if (FAILED(write->GetSystemFontCollection(FALSE, collection.GetAddressOf(), FALSE))) return false;
        BOOL exists = FALSE;
        if (choice.source == FontSource::defaults) {
            collection->FindFamilyName(L"Microsoft YaHei UI", &familyIndex, &exists);
            if (!exists) collection->FindFamilyName(L"Segoe UI", &familyIndex, &exists);
        } else if (FAILED(collection->FindFamilyName(choice.value.c_str(), &familyIndex, &exists)) || !exists) {
            return false;
        }
        if (!exists) familyIndex = 0;
    }

    ComPtr<IDWriteFontFamily1> family;
    ComPtr<IDWriteFont> regular;
    ComPtr<IDWriteLocalizedStrings> names;
    ComPtr<IDWriteGdiInterop> gdi;
    BOOL isSystemFont = FALSE;
    if (FAILED(collection->GetFontFamily(familyIndex, family.GetAddressOf()))
        || FAILED(family->GetFirstMatchingFont(DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
            DWRITE_FONT_STYLE_NORMAL, regular.GetAddressOf()))
        || FAILED(family->GetFamilyNames(names.GetAddressOf()))
        || FAILED(write->GetGdiInterop(gdi.GetAddressOf()))
        || FAILED(gdi->ConvertFontToLOGFONT(regular.Get(), &face, &isSystemFont))) {
        return false;
    }

    UINT32 nameIndex = 0;
    BOOL named = FALSE;
    if (FAILED(names->FindLocaleName(L"en-us", &nameIndex, &named)) || !named) nameIndex = 0;
    UINT32 length = 0;
    if (FAILED(names->GetStringLength(nameIndex, &length))) return false;
    std::wstring name(length + 1, L'\0');
    if (FAILED(names->GetString(nameIndex, name.data(), length + 1))
        || FAILED(write->CreateTextFormat(name.c_str(), collection.Get(), regular->GetWeight(),
            regular->GetStyle(), regular->GetStretch(), 13.f, i18n::LocaleName(), format))) return false;
    const DWRITE_TRIMMING trimming{DWRITE_TRIMMING_GRANULARITY_CHARACTER, 0, 0};
    ComPtr<IDWriteInlineObject> ellipsis;
    return SUCCEEDED(write->CreateEllipsisTrimmingSign(*format, ellipsis.GetAddressOf()))
        && SUCCEEDED((*format)->SetTrimming(&trimming, ellipsis.Get()))
        && SUCCEEDED((*format)->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP))
        && SUCCEEDED((*format)->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER));
}

HRESULT CreateCaretVisual(IDCompositionDesktopDevice& device, IDCompositionVisual2& parent,
    IDCompositionVisual3** caret)
{
    ComPtr<IDCompositionSurface> pixel;
    HRESULT result = device.CreateSurface(1, 1, DXGI_FORMAT_B8G8R8A8_UNORM,
        DXGI_ALPHA_MODE_PREMULTIPLIED, pixel.GetAddressOf());
    if (FAILED(result)) return result;

    POINT offset{};
    ComPtr<ID2D1DeviceContext> canvas;
    result = pixel->BeginDraw(nullptr, IID_PPV_ARGS(canvas.GetAddressOf()), &offset);
    if (FAILED(result)) return result;
    canvas->SetDpi(96, 96);
    canvas->SetTransform(D2D1::Matrix3x2F::Identity());
    const auto x = static_cast<float>(offset.x);
    const auto y = static_cast<float>(offset.y);
    canvas->PushAxisAlignedClip(D2D1::RectF(x, y, x + 1, y + 1),
        D2D1_ANTIALIAS_MODE_ALIASED);
    canvas->Clear(D2D1::ColorF(0x000000));
    canvas->PopAxisAlignedClip();
    canvas.Reset();
    result = pixel->EndDraw();
    if (FAILED(result)) return result;

    ComPtr<IDCompositionVisual2> base;
    result = device.CreateVisual(base.GetAddressOf());
    if (FAILED(result)) return result;
    ComPtr<IDCompositionVisual3> overlay;
    result = base.As(&overlay);
    if (FAILED(result)) return result;
    result = overlay->SetContent(pixel.Get());
    if (FAILED(result)) return result;
    result = overlay->SetVisible(FALSE);
    if (FAILED(result)) return result;
    result = parent.AddVisual(overlay.Get(), TRUE, nullptr);
    if (FAILED(result)) return result;
    return overlay.CopyTo(caret);
}

HRESULT InitDrawingDevice()
{
    if (drawingDevice) return S_OK;
    YISOU_PERF_MEMORY_SCOPE("init_drawing_device");
    ComPtr<ID3D11Device> graphics;
    // This small, event-driven window costs less memory without loading the hardware driver.
    HRESULT result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_PREVENT_INTERNAL_THREADING_OPTIMIZATIONS,
        nullptr, 0, D3D11_SDK_VERSION, graphics.GetAddressOf(), nullptr, nullptr);
    if (FAILED(result)) {
        result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr, 0, D3D11_SDK_VERSION, graphics.ReleaseAndGetAddressOf(), nullptr, nullptr);
    }
    if (FAILED(result)) return result;

    ComPtr<IDXGIDevice> dxgi;
    result = graphics.As(&dxgi);
    if (FAILED(result)) return result;
    result = drawingFactory->CreateDevice(dxgi.Get(), drawingDevice.GetAddressOf());
    if (FAILED(result)) return result;
    drawingDevice->SetMaximumTextureMemory(512 * 1024);
    dxgi.As(&dxgiDevice);
    SetIconDevice(drawingDevice.Get());
    return S_OK;
}

HRESULT InitComposition(HWND window)
{
    YISOU_PERF_MEMORY_SCOPE("init_composition");
    HRESULT result = InitDrawingDevice();
    if (FAILED(result)) return result;
    ComPtr<IDCompositionDesktopDevice> newComposition;
    result = DCompositionCreateDevice2(drawingDevice.Get(), IID_PPV_ARGS(newComposition.GetAddressOf()));
    if (FAILED(result)) return result;
    ComPtr<IDCompositionTarget> newTarget;
    result = newComposition->CreateTargetForHwnd(window, TRUE, newTarget.GetAddressOf());
    if (FAILED(result)) return result;
    ComPtr<IDCompositionVisual2> newVisual;
    result = newComposition->CreateVisual(newVisual.GetAddressOf());
    if (FAILED(result)) return result;
    ComPtr<IDCompositionVisual3> newCaret;
    result = CreateCaretVisual(*newComposition.Get(), *newVisual.Get(), newCaret.GetAddressOf());
    if (FAILED(result)) return result;
    result = newTarget->SetRoot(newVisual.Get());
    if (FAILED(result)) return result;

    composition = newComposition;
    compositionTarget = newTarget;
    visual = newVisual;
    caretVisual = newCaret;
    return S_OK;
}

void ReleaseComposition()
{
    YISOU_PERF_MEMORY_SCOPE("release_composition");
    SetIconDevice(nullptr);
    engineBitmap.Reset();
    for (auto& bitmap : chevronBitmaps) bitmap.Reset();
    surface.Reset();
    caretVisual.Reset();
    visual.Reset();
    compositionTarget.Reset();
    composition.Reset();
    drawingDevice.Reset();
    dxgiDevice.Reset();
    surfaceSize = {};
}

void ScheduleGraphicsCleanup(HWND window)
{
    if (!window || !drawingDevice) return;
    const UINT delay = IsWindowVisible(window) || HasOpenSettings() ? 250 : graphicsIdleDelay;
    SetTimer(window, graphicsIdleTimer, delay, nullptr);
}

HRESULT PositionCaret()
{
    POINT position{};
    if (GetFocus() != edit || !GetCaretPos(&position)) return caretVisual->SetVisible(FALSE);

    const int pixelWidth = std::max(1, MulDiv(3, dpi, USER_DEFAULT_SCREEN_DPI * 2));
    const int caretX = compositionCaretX.value_or(static_cast<int>(position.x));
    const int x = Scale(editLeft) + std::clamp(caretX, 0, Scale(editWidth) - pixelWidth);
    const int pixelHeight = Scale(caretHeight);
    const int y = (Scale(height) - pixelHeight) / 2;
    const auto transform = D2D1::Matrix3x2F::Scale(static_cast<float>(pixelWidth),
        static_cast<float>(pixelHeight))
        * D2D1::Matrix3x2F::Translation(static_cast<float>(x), static_cast<float>(y));
    const HRESULT result = static_cast<IDCompositionVisual*>(caretVisual.Get())->SetTransform(transform);
    return SUCCEEDED(result) ? caretVisual->SetVisible(caretVisible) : result;
}

void UpdateCompositionText(HWND input)
{
    compositionText.clear();
    compositionCursor = 0;
    const HIMC context = ImmGetContext(input);
    if (!context) return;

    const LONG length = ImmGetCompositionStringW(context, GCS_COMPSTR, nullptr, 0);
    if (length > 0) {
        compositionText.resize(length / sizeof(wchar_t));
        const LONG received = ImmGetCompositionStringW(context, GCS_COMPSTR, compositionText.data(), length);
        compositionText.resize(std::max(0L, received) / sizeof(wchar_t));
        const LONG cursor = ImmGetCompositionStringW(context, GCS_CURSORPOS, nullptr, 0);
        compositionCursor = cursor < 0
            ? compositionText.size()
            : std::min(static_cast<size_t>(cursor), compositionText.size());
    }
    ImmReleaseContext(input, context);
}

int MeasureText(HDC dc, std::wstring_view text)
{
    SIZE size{};
    GetTextExtentPoint32W(dc, text.data(), static_cast<int>(text.size()), &size);
    return size.cx;
}

void PaintCompositionText(HDC dc)
{
    // Keep the committed text in the native edit so IME selection and undo stay native.
    const int length = GetWindowTextLengthW(edit);
    std::wstring text(length + 1, L'\0');
    text.resize(GetWindowTextW(edit, text.data(), length + 1));

    DWORD first{}, last{};
    SendMessageW(edit, EM_GETSEL, reinterpret_cast<WPARAM>(&first), reinterpret_cast<LPARAM>(&last));
    const auto start = std::min(static_cast<size_t>(first), text.size());
    const auto end = std::clamp(static_cast<size_t>(last), start, text.size());
    text.replace(start, end - start, compositionText);

    RECT line{};
    SendMessageW(edit, EM_GETRECT, 0, reinterpret_cast<LPARAM>(&line));
    POINT anchor{line.left, line.top};
    GetCaretPos(&anchor);
    const LRESULT position = SendMessageW(edit, EM_POSFROMCHAR, start, 0);
    if (position != -1) anchor.x = GET_X_LPARAM(position);

    const HGDIOBJ previous = SelectObject(dc, editFont ? editFont : GetStockObject(DEFAULT_GUI_FONT));
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, RGB(26, 32, 41));
    const std::wstring_view preview(text);
    const int startWidth = MeasureText(dc, preview.substr(0, start));
    const int endWidth = MeasureText(dc, preview.substr(0, start + compositionText.size()));
    const int cursorWidth = MeasureText(dc, preview.substr(0, start + compositionCursor));
    const int x = std::min(static_cast<int>(anchor.x) - startWidth,
        static_cast<int>(line.right) - cursorWidth - Scale(2));
    ExtTextOutW(dc, x, line.top, ETO_CLIPPED, &line, text.data(), static_cast<UINT>(text.size()), nullptr);
    compositionCaretX = x + cursorWidth;

    TEXTMETRICW metrics{};
    if (GetTextMetricsW(dc, &metrics)) {
        const LONG y = std::min(line.top + metrics.tmAscent + Scale(2), line.bottom - 1);
        const RECT underline{std::max<LONG>(line.left, x + startWidth), y,
            std::min<LONG>(line.right, x + endWidth), y + 1};
        if (underline.left < underline.right) {
            SetDCBrushColor(dc, RGB(93, 93, 93));
            FillRect(dc, &underline, reinterpret_cast<HBRUSH>(GetStockObject(DC_BRUSH)));
        }
    }
    SelectObject(dc, previous);
}

void PaintEdit(ID2D1DeviceContext& canvas)
{
    YISOU_PERF_SCOPE("paint_input");
    compositionCaretX.reset();
    RECT bounds{};
    GetClientRect(edit, &bounds);

    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = bounds.right;
    info.bmiHeader.biHeight = -bounds.bottom;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    const HBITMAP dib = CreateDIBSection(nullptr, &info, DIB_RGB_COLORS, &bits, nullptr, 0);
    if (!dib) return;
    const HDC dc = CreateCompatibleDC(nullptr);
    if (!dc) {
        DeleteObject(dib);
        return;
    }
    const HGDIOBJ previous = SelectObject(dc, dib);

    FillRect(dc, &bounds, reinterpret_cast<HBRUSH>(GetStockObject(WHITE_BRUSH)));
    if (compositionText.empty()) {
        SendMessageW(edit, WM_PRINTCLIENT, reinterpret_cast<WPARAM>(dc), PRF_CLIENT | PRF_ERASEBKGND);
    } else {
        PaintCompositionText(dc);
    }
    GdiFlush();

    // Keep the native edit glyphs and selection while removing the white matte.
    auto* pixels = static_cast<RGBQUAD*>(bits);
    const auto count = static_cast<size_t>(bounds.right) * bounds.bottom;
    for (size_t index = 0; index < count; ++index) {
        auto& pixel = pixels[index];
        const BYTE matte = std::min({pixel.rgbRed, pixel.rgbGreen, pixel.rgbBlue});
        pixel.rgbRed -= matte;
        pixel.rgbGreen -= matte;
        pixel.rgbBlue -= matte;
        pixel.rgbReserved = static_cast<BYTE>(255 - matte);
    }

    const auto properties = D2D1::BitmapProperties(
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED), 96, 96);
    ComPtr<ID2D1Bitmap> bitmap;
    if (SUCCEEDED(canvas.CreateBitmap(D2D1::SizeU(bounds.right, bounds.bottom), pixels,
        bounds.right * sizeof(RGBQUAD), &properties, bitmap.GetAddressOf()))) {
        const auto x = static_cast<float>(Scale(editLeft));
        const auto y = static_cast<float>(editTopPixels);
        const auto size = bitmap->GetSize();
        canvas.DrawBitmap(bitmap.Get(), D2D1::RectF(x, y, x + size.width, y + size.height), 1.f,
            D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR);
    }
    SelectObject(dc, previous);
    DeleteDC(dc);
    DeleteObject(dib);
}

void CreateIconBitmap(ID2D1DeviceContext& canvas, std::span<const std::uint32_t> pixels, unsigned size,
    ComPtr<ID2D1Bitmap>& bitmap)
{
    if (pixels.empty()) return;
    const auto properties = D2D1::BitmapProperties(
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED), 96, 96);
    canvas.CreateBitmap(D2D1::SizeU(size, size), pixels.data(), size * sizeof(std::uint32_t),
        properties, bitmap.ReleaseAndGetAddressOf());
}

void DrawIconBitmap(ID2D1DeviceContext& canvas, ID2D1Bitmap& bitmap, int x, int y, unsigned edge)
{
    const float left = static_cast<float>(x);
    const float top = static_cast<float>(y);
    const float size = static_cast<float>(edge);
    const auto source = bitmap.GetPixelSize();
    const bool nativeSize = source.width == edge && source.height == edge;
    canvas.DrawBitmap(&bitmap, D2D1::RectF(left, top, left + size, top + size), 1.f,
        nativeSize ? D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR : D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
}

void PaintEngine(ID2D1DeviceContext& canvas, ID2D1SolidColorBrush& brush, const D2D1_MATRIX_3X2_F& pixelTransform)
{
    const auto face = D2D1::RoundedRect(D2D1::RectF(
        engineLeft + 1, engineTop + 1, engineLeft + engineWidth - 1, engineTop + engineHeight - 1), 8, 8);
    const bool active = engineFlyout || enginePressed;
    if (active || engineHovered) {
        const auto color = active ? panelSelection : panelHover;
        brush.SetColor(D2D1::ColorF(GetRValue(color) / 255.f, GetGValue(color) / 255.f, GetBValue(color) / 255.f, .88f));
        canvas.FillRoundedRectangle(face, &brush);
    }
    if (GetFocus() == engineButton && !engineFlyout) {
        brush.SetColor(D2D1::ColorF(accentColor));
        canvas.DrawRoundedRectangle(face, &brush);
    }

    const auto* preview = application->findEngine(displayedEngine);
    const auto& engine = preview ? *preview : application->activeEngine();
    const auto size = static_cast<unsigned>(Scale(engineIconSize));
    if (!engineBitmap || bitmapEngine != engine.id) {
        CreateIconBitmap(canvas, EngineIcon(engine, size), engine.icon.empty() ? size : iconSize, engineBitmap);
        bitmapEngine = engine.id;
    }

    brush.SetColor(D2D1::ColorF(textColor));
    const std::wstring_view name = i18n::EngineName(engine.id, engine.name.c_str());
    canvas.DrawText(name.data(), static_cast<UINT32>(name.size()), engineFormat.Get(),
        D2D1::RectF(engineLeft + 33, engineTop, engineLeft + engineWidth - 25, engineTop + engineHeight), &brush,
        D2D1_DRAW_TEXT_OPTIONS_CLIP);

    auto& chevron = chevronBitmaps[engineFlyout ? 1 : 0];
    const auto chevronSize = static_cast<unsigned>(Scale(engineChevronSize));
    if (!chevron) {
        const auto icon = engineFlyout ? TablerIcon::chevronUp : TablerIcon::chevronDown;
        constexpr auto color = RGB((mutedColor >> 16) & 255, (mutedColor >> 8) & 255, mutedColor & 255);
        CreateIconBitmap(canvas, TablerPixels(icon, color, chevronSize), chevronSize, chevron);
    }

    D2D1_MATRIX_3X2_F layoutTransform{};
    canvas.GetTransform(&layoutTransform);
    canvas.SetTransform(pixelTransform);
    if (engineBitmap) {
        DrawIconBitmap(canvas, *engineBitmap.Get(), Scale(engineLeft + 8), Scale(engineTop + 7), size);
    }
    if (chevron) {
        DrawIconBitmap(canvas, *chevron.Get(), Scale(engineLeft + engineWidth - 23), Scale(engineTop + 9), chevronSize);
    }
    canvas.SetTransform(layoutTransform);
}

HRESULT Paint(HWND window)
{
    YISOU_PERF_SCOPE("paint_search");
    RECT bounds{};
    GetClientRect(window, &bounds);
    if (bounds.right == 0 || bounds.bottom == 0) return S_OK;
    if (!surface || surfaceSize.cx != bounds.right || surfaceSize.cy != bounds.bottom) {
        const HRESULT created = composition->CreateSurface(bounds.right, bounds.bottom, DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_ALPHA_MODE_PREMULTIPLIED, surface.ReleaseAndGetAddressOf());
        if (FAILED(created)) return created;
        const HRESULT attached = visual->SetContent(surface.Get());
        if (FAILED(attached)) {
            surface.Reset();
            return attached;
        }
        surfaceSize = {bounds.right, bounds.bottom};
    }

    POINT offset{};
    ComPtr<ID2D1DeviceContext> canvas;
    HRESULT result = surface->BeginDraw(nullptr, IID_PPV_ARGS(canvas.GetAddressOf()), &offset);
    if (FAILED(result)) return result;

    const float factor = static_cast<float>(dpi) / USER_DEFAULT_SCREEN_DPI;
    const auto translation = D2D1::Matrix3x2F::Translation(static_cast<float>(offset.x), static_cast<float>(offset.y));
    const auto clientRect = D2D1::RectF(0, 0, static_cast<float>(bounds.right), static_cast<float>(bounds.bottom));
    canvas->SetDpi(96, 96);
    canvas->SetTransform(translation);
    canvas->SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
    canvas->PushAxisAlignedClip(clientRect, D2D1_ANTIALIAS_MODE_ALIASED);
    canvas->Clear(D2D1::ColorF(0x000000, 0.f));

    ComPtr<ID2D1SolidColorBrush> brush;
    if (SUCCEEDED(canvas->CreateSolidColorBrush(D2D1::ColorF(fallbackColor, backdropEnabled ? .68f : 1.f),
        brush.GetAddressOf()))) {
        const auto radius = cardRadius * factor;
        canvas->FillRoundedRectangle(D2D1::RoundedRect(clientRect, radius, radius), brush.Get());
        const auto frame = D2D1::RectF(.5f, .5f, clientRect.right - .5f, clientRect.bottom - .5f);
        brush->SetColor(D2D1::ColorF(0x233953, .16f));
        canvas->DrawRoundedRectangle(D2D1::RoundedRect(frame, radius, radius), brush.Get());
        PaintEdit(*canvas.Get());
        canvas->SetTransform(D2D1::Matrix3x2F::Scale(factor, factor) * translation);
        PaintEngine(*canvas.Get(), *brush.Get(), translation);
        if (browserError || linkAvailable) {
            const std::wstring_view hint = browserError ? Tr(Text::browserErrorHint)
                : Tr(Text::openLinkHint);
            brush->SetColor(D2D1::ColorF(browserError ? 0x9d3026 : mutedColor));
            canvas->DrawText(hint.data(), static_cast<UINT32>(hint.size()), engineFormat.Get(),
                D2D1::RectF(editLeft, height - 4.f, width - editLeft, WindowHeight() - 7.f), brush.Get());
        }
    }
    canvas->PopAxisAlignedClip();
    canvas.Reset();
    result = surface->EndDraw();
    if (SUCCEEDED(result)) result = PositionCaret();
    return SUCCEEDED(result) ? composition->Commit() : result;
}

void RestartCaret(HWND window)
{
    caretVisible = true;
    const UINT interval = GetCaretBlinkTime();
    if (interval && interval != INFINITE) SetTimer(window, caretTimer, interval, nullptr);
    InvalidateRect(window, nullptr, FALSE);
}

HFONT CreateUiFont(const LOGFONTW& face, int fontSize)
{
    LOGFONTW settings = face;
    settings.lfHeight = -Scale(fontSize);
    settings.lfWidth = 0;
    settings.lfQuality = ANTIALIASED_QUALITY;
    return CreateFontIndirectW(&settings);
}

int EditTextCenter()
{
    const HDC dc = GetDC(edit);
    if (!dc) return Scale(editHeight) / 2;

    const HGDIOBJ font = editFont ? editFont : GetStockObject(DEFAULT_GUI_FONT);
    const HGDIOBJ previous = SelectObject(dc, font);
    TEXTMETRICW metrics{};
    GLYPHMETRICS glyph{};
    const MAT2 identity{{0, 1}, {}, {}, {0, 1}};
    WORD glyphIndex = 0xffff;
    GetGlyphIndicesW(dc, L"中", 1, &glyphIndex, GGI_MARK_NONEXISTING_GLYPHS);
    const auto reference = glyphIndex == 0xffff ? L'H' : L'中';
    const bool measured = GetTextMetricsW(dc, &metrics)
        && GetGlyphOutlineW(dc, reference, GGO_METRICS, &glyph, 0, nullptr, &identity) != GDI_ERROR;
    SelectObject(dc, previous);
    ReleaseDC(edit, dc);
    if (!measured) return Scale(editHeight) / 2;

    RECT line{};
    SendMessageW(edit, EM_GETRECT, 0, reinterpret_cast<LPARAM>(&line));
    // Center the visible ink of a representative glyph on a stable baseline.
    return line.top + metrics.tmAscent - glyph.gmptGlyphOrigin.y + static_cast<int>(glyph.gmBlackBoxY) / 2;
}

void SetEditFont(HFONT input)
{
    if (!input) return;
    SendMessageW(edit, WM_SETFONT, reinterpret_cast<WPARAM>(input), FALSE);
    if (editFont) DeleteObject(editFont);
    editFont = input;
}

void LayoutControls()
{
    const int centeredTop = (Scale(height) - Scale(editHeight)) / 2;
    MoveWindow(edit, Scale(editLeft), centeredTop, Scale(editWidth), Scale(editHeight), FALSE);
    editTopPixels = Scale(height) / 2 - EditTextCenter();
    SetWindowPos(edit, nullptr, Scale(editLeft), editTopPixels, 0, 0, SWP_NOZORDER | SWP_NOSIZE | SWP_NOACTIVATE);
    MoveWindow(engineButton, Scale(engineLeft), Scale(engineTop), Scale(engineWidth), Scale(engineHeight), TRUE);
}

bool ApplyFont(const FontChoice& choice, bool save)
{
    if (engineFormat && activeFont == choice) return !save || application->changeFont(choice);
    YISOU_PERF_MEMORY_SCOPE("apply_font");

    LOGFONTW face{};
    ComPtr<IDWriteTextFormat> format;
    if (!ResolveFont(choice, face, format.GetAddressOf())) return false;
    const bool isFile = choice.source == FontSource::file;
    if (isFile && !AddFontResourceExW(choice.value.c_str(), privateFontFlags, nullptr)) return false;

    const HFONT input = edit ? CreateUiFont(face, inputFontSize) : nullptr;
    if ((edit && !input) || (save && !application->changeFont(choice))) {
        if (input) DeleteObject(input);
        if (isFile) RemoveFontResourceExW(choice.value.c_str(), privateFontFlags, nullptr);
        return false;
    }

    const auto previous = std::move(activeFont);
    activeFont = choice;
    uiFontTemplate = face;
    engineFormat = std::move(format);
    SetEditFont(input);
    if (previous.source == FontSource::file) {
        RemoveFontResourceExW(previous.value.c_str(), privateFontFlags, nullptr);
    }
    if (edit) {
        LayoutControls();
        InvalidateRect(GetParent(edit), nullptr, FALSE);
    }
    return true;
}

bool PrepareUiFonts()
{
    if (!drawingFactory) {
        YISOU_PERF_MEMORY_SCOPE("init_direct2d");
        if (FAILED(D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, drawingFactory.GetAddressOf()))) return false;
    }
    if (!write) {
        YISOU_PERF_MEMORY_SCOPE("init_directwrite");
        if (FAILED(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory3),
            reinterpret_cast<IUnknown**>(write.GetAddressOf())))) return false;
    }
    return engineFormat || ApplyFont(application->preferences().font, false) || ApplyFont({}, false);
}

bool PrepareSearchUi(HWND window)
{
    return PrepareUiFonts() && (composition || SUCCEEDED(InitComposition(window)));
}

std::wstring ReadSearchText()
{
    const int length = GetWindowTextLengthW(edit);
    std::wstring value(length + 1, L'\0');
    value.resize(GetWindowTextW(edit, value.data(), length + 1));
    return value;
}

void UpdateEngineName()
{
    if (!engineButton) return;
    const auto value = ReadSearchText();
    const auto input = application->parseInput(literalInput ? L"" : std::wstring_view(value), temporaryEngine);
    displayedEngine = input.engine->id;
    const auto* name = i18n::EngineName(input.engine->id, input.engine->name.c_str());
    if (displayedEngineName != name) {
        displayedEngineName = name;
        SetWindowTextW(engineButton, i18n::Format(Text::engineButton, {displayedEngineName}).c_str());
    }
    InvalidateRect(GetParent(engineButton), nullptr, FALSE);
}

bool ApplySettingsFont(const FontChoice& choice, LOGFONTW& face)
{
    if (!ApplyFont(choice, true)) return false;
    face = uiFontTemplate;
    return true;
}

bool ApplySettingsEngine(const EngineId& engine)
{
    if (!application->changeEngine(engine)) return false;
    UpdateEngineName();
    UpdateSettingsEngine();
    return true;
}

bool SaveShortcutPreference(Shortcut shortcut)
{
    return application->changeShortcut(shortcut);
}

platform::ShortcutResult ApplySettingsShortcut(Shortcut shortcut)
{
    if (!shortcutRegistration) return platform::ShortcutResult::unavailable;
    const auto& binding = application->preferences().selectionShortcut;
    if (binding && binding->shortcut == shortcut) return platform::ShortcutResult::unavailable;
    const auto result = shortcutRegistration->apply(shortcut, SaveShortcutPreference);
    if (result == platform::ShortcutResult::applied) shortcutActive = true;
    return result;
}

void SetShortcutRecording(bool recording)
{
    if (shortcutRegistration) shortcutRegistration->setRecording(recording);
}

platform::ShortcutResult ApplyClipboardShortcut(const std::optional<Shortcut>& shortcut)
{
    using platform::ShortcutResult;
    if (!clipboardRegistration) return ShortcutResult::unavailable;
    if (!shortcut) {
        if (!application->changeClipboardShortcut(std::nullopt)) return ShortcutResult::saveFailed;
        clipboardRegistration->clear();
        clipboardShortcutActive = false;
        return ShortcutResult::applied;
    }
    if (!shortcut->key) return ShortcutResult::unavailable;
    const auto result = clipboardRegistration->apply(*shortcut,
        [](Shortcut binding) { return application->changeClipboardShortcut(binding); });
    if (result == ShortcutResult::applied) clipboardShortcutActive = true;
    return result;
}

platform::ShortcutResult ApplySelectionShortcut(const std::optional<SelectionShortcut>& binding)
{
    using platform::ShortcutResult;
    if (!selectionRegistration) return ShortcutResult::unavailable;
    if (!binding) {
        if (!application->changeSelectionShortcut(std::nullopt)) return ShortcutResult::saveFailed;
        selectionRegistration->clear();
        selectionShortcutActive = false;
        return ShortcutResult::applied;
    }
    if (!binding->shortcut.key || binding->shortcut == application->preferences().shortcut) return ShortcutResult::unavailable;
    const auto result = selectionRegistration->apply(binding->shortcut,
        [&binding](Shortcut) { return application->changeSelectionShortcut(binding); });
    if (result == ShortcutResult::applied) selectionShortcutActive = true;
    return result;
}

void OpenSettings(HWND window)
{
    YISOU_PERF_MEMORY_SCOPE("open_settings");
    if (!PrepareUiFonts() || FAILED(InitDrawingDevice())) return;
    KillTimer(window, graphicsIdleTimer);
    ShowSettings(window, *application, *drawingFactory.Get(), {activeFont, uiFontTemplate},
        {ApplySettingsFont, ApplySettingsEngine, ApplySettingsShortcut, SetShortcutRecording,
            [] { return shortcutActive; }, [] {
                if (selectionRegistration && !application->preferences().selectionShortcut) {
                    selectionRegistration->clear();
                    selectionShortcutActive = false;
                }
                engineBitmap.Reset();
                UpdateEngineName();
            }, ApplySelectionShortcut, [] { return selectionShortcutActive; }, ScheduleGraphicsCleanup,
            [](HWND owner) { PostMessageW(owner, restartConfiguration, 0, 0); },
            ApplyClipboardShortcut, [] { return clipboardShortcutActive; },
            [](HWND owner) { PostMessageW(owner, restartConfiguration, restartForLanguage, 0); }});
    ScheduleGraphicsCleanup(window);
}

void OnFlyoutClosed(HWND flyout)
{
    if (engineFlyout != flyout) return;
    engineFlyout = nullptr;
    const HWND window = GetParent(engineButton);
    InvalidateRect(window, nullptr, FALSE);
    ScheduleGraphicsCleanup(window);
}

void OnSuggestionClosed(HWND flyout)
{
    if (suggestionFlyout == flyout) suggestionFlyout = nullptr;
}

void AcceptEngineSuggestion(HWND, const EngineId& engine)
{
    temporaryEngine = engine;
    SetWindowTextW(edit, L"");
    UpdateEngineName();
}

void UpdateEngineSuggestions()
{
    if (!edit || !IsWindowVisible(edit) || GetFocus() != edit || composing || literalInput || engineFlyout) {
        if (suggestionFlyout) DestroyWindow(suggestionFlyout);
        return;
    }
    const auto suggestions = application->suggestEngines(ReadSearchText(), i18n::EngineName);
    if (suggestions.empty()) {
        if (suggestionFlyout) DestroyWindow(suggestionFlyout);
        return;
    }
    if (suggestionFlyout && RefreshEngineSuggestions(suggestionFlyout, suggestions)) return;
    if (suggestionFlyout) DestroyWindow(suggestionFlyout);
    suggestionFlyout = CreateEngineFlyout(edit, *application, *drawingFactory.Get(), *write.Get(), *engineFormat.Get(),
        {AcceptEngineSuggestion, nullptr, OnSuggestionClosed}, displayedEngine, &suggestions);
    if (suggestionFlyout) ShowEngineFlyout(suggestionFlyout);
}

bool HandleSearchSuggestionKey(const MSG& message)
{
    if (!suggestionFlyout || message.hwnd != edit || message.message != WM_KEYDOWN || composing) return false;
    if (platform::ShortcutModifiers()) return false;
    return HandleEngineSuggestionKey(suggestionFlyout, message.wParam);
}

void SelectFlyoutEngine(HWND window, const EngineId& engine)
{
    const auto value = ReadSearchText();
    const auto input = application->parseInput(literalInput ? L"" : std::wstring_view(value));
    if (input.usesAlias) {
        const std::wstring query(input.query);
        SetWindowTextW(edit, query.c_str());
    }
    temporaryEngine = engine;
    UpdateEngineName();
    if (GetForegroundWindow() == window) SetFocus(edit);
}

void OpenFlyoutSettings(HWND window, bool add)
{
    OpenSettings(window);
    ShowEngineSettingsPage(add);
}

void ToggleEngineFlyout(HWND window)
{
    YISOU_PERF_MEMORY_SCOPE("toggle_engine_flyout");
    if (suggestionFlyout) DestroyWindow(suggestionFlyout);
    if (engineFlyout) {
        DestroyWindow(engineFlyout);
        return;
    }
    engineFlyout = CreateEngineFlyout(engineButton, *application, *drawingFactory.Get(), *write.Get(),
        *engineFormat.Get(), {SelectFlyoutEngine, OpenFlyoutSettings, OnFlyoutClosed}, displayedEngine);
    if (!engineFlyout) return;
    InvalidateRect(window, nullptr, FALSE);
    ShowEngineFlyout(engineFlyout);
    ScheduleGraphicsCleanup(window);
}

void UpdateSearchNotice(HWND window, bool error, bool link)
{
    if (browserError == error && linkAvailable == link) return;
    const int previousHeight = WindowHeight();
    browserError = error;
    linkAvailable = link;
    if (previousHeight != WindowHeight()) {
        SetWindowPos(window, nullptr, 0, 0, Scale(width), Scale(WindowHeight()), SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOMOVE);
    }
    InvalidateRect(window, nullptr, FALSE);
}

void UpdateBrowserError(HWND window, bool error)
{
    UpdateSearchNotice(window, error, linkAvailable);
}

void RevealSearch(HWND window, HWND source = nullptr)
{
    YISOU_PERF_MEMORY_SCOPE("reveal_search");
    if (!PrepareSearchUi(window)) {
        ScheduleGraphicsCleanup(window);
        return;
    }
    KillTimer(window, graphicsIdleTimer);
    if (!IsWindowVisible(window) || (source && MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) != TargetMonitor(source))) {
        PositionSearch(window, TargetMonitor(source ? source : GetForegroundWindow()));
    }
    if (!IsWindowVisible(window)) ShowWindow(window, SW_SHOWNOACTIVATE);
    if (!SetForegroundWindow(window)) {
        const HWND foreground = GetForegroundWindow();
        const auto foregroundThread = GetWindowThreadProcessId(foreground, nullptr);
        const auto thread = GetCurrentThreadId();
        if (foregroundThread && foregroundThread != thread && !IsHungAppWindow(foreground)
            && AttachThreadInput(thread, foregroundThread, TRUE)) {
            SetForegroundWindow(window);
            AttachThreadInput(thread, foregroundThread, FALSE);
        }
    }
    if (GetForegroundWindow() == window) SetFocus(edit);
    ScheduleGraphicsCleanup(window);
}

void Search(HWND window)
{
    const auto value = ReadSearchText();
    const auto input = application->parseInput(literalInput ? L"" : std::wstring_view(value), temporaryEngine);
    const auto result = application->search(literalInput ? std::wstring_view(value) : input.query, input.engine,
        literalInput ? literalOrigin : SearchOrigin::typed, retrySource);
    UpdateBrowserError(window, result == SearchResult::openFailed);
    if (result == SearchResult::opened) {
        lastQuery = literalInput ? value : std::wstring(input.query);
        temporaryEngine.clear();
        literalInput = false;
        previewInput = false;
        retrySource.clear();
        SetWindowTextW(edit, L"");
        UpdateEngineName();
    } else SetFocus(edit);
}

std::wstring SourceExecutable(HWND source)
{
    DWORD processId = 0;
    GetWindowThreadProcessId(source, &processId);
    const HANDLE process = processId ? OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, processId) : nullptr;
    if (!process) return {};
    wchar_t path[32768]{};
    DWORD length = static_cast<DWORD>(std::size(path));
    const bool found = QueryFullProcessImageNameW(process, 0, path, &length) != FALSE;
    CloseHandle(process);
    return found ? std::wstring(path, length) : std::wstring{};
}

bool IsDoubleAltExcluded(std::wstring_view executable, const std::vector<std::wstring>& excluded)
{
    const auto separator = executable.find_last_of(L"\\/");
    const auto name = separator == executable.npos ? executable : executable.substr(separator + 1);
    return !name.empty() && std::ranges::any_of(excluded, [&](const std::wstring& item) {
        return CompareStringOrdinal(name.data(), static_cast<int>(name.size()), item.c_str(), -1, TRUE) == CSTR_EQUAL;
    });
}

std::wstring NormalizeSearchText(std::wstring_view text)
{
    std::wstring query;
    query.reserve(std::min<size_t>(platform::selectionLimit, text.size()));
    for (const wchar_t ch : text) {
        if (ch == L' ' || ch == L'\r' || ch == L'\n' || ch == L'\t') {
            if (!query.empty() && query.back() != L' ') query += L' ';
        } else query += ch;
        if (query.size() == platform::selectionLimit) break;
    }
    if (!query.empty() && (query.back() == L' ' || (query.back() >= 0xd800 && query.back() <= 0xdbff))) query.pop_back();
    return query;
}

void FinishSelection(HWND window, const std::wstring& text, HWND source = nullptr, const EngineId& engineId = {},
    bool preview = false)
{
    const auto* engine = engineId.empty() ? nullptr : application->findEngine(engineId);
    if (!engineId.empty() && !engine) return;
    const bool hasText = text.find_first_not_of(L" \t\r\n") != std::wstring::npos;
    if (!hasText && engine && !preview) return;
    const auto label = hasText && source && (preview || application->preferences().recordHistory)
        ? platform::DescribeBrowser(SourceExecutable(source)).name : std::wstring{};
    if (preview) {
        const auto query = NormalizeSearchText(text);
        temporaryEngine = engineId;
        literalInput = true;
        literalOrigin = SearchOrigin::selection;
        previewInput = true;
        retrySource = label;
        SetWindowTextW(edit, query.c_str());
        SendMessageW(edit, EM_SETSEL, query.size(), query.size());
        UpdateBrowserError(window, false);
        UpdateEngineName();
        RevealSearch(window, source);
        return;
    }
    const auto result = application->search(text, engine, SearchOrigin::selection, label);
    if (result == SearchResult::opened) {
        lastQuery = NormalizeSearchText(text);
        return;
    }
    if (engine && result == SearchResult::emptyQuery) return;
    if (result == SearchResult::openFailed) {
        temporaryEngine = engineId;
        literalInput = true;
        literalOrigin = SearchOrigin::selection;
        previewInput = false;
        retrySource = label;
        SetWindowTextW(edit, text.c_str());
        UpdateEngineName();
    }
    UpdateBrowserError(window, result == SearchResult::openFailed);
    RevealSearch(window, source);
}

void SearchClipboard(HWND window, std::wstring text)
{
    const auto result = application->search(text, nullptr, SearchOrigin::clipboard);
    if (result == SearchResult::opened) {
        lastQuery = std::move(text);
        return;
    }
    if (result == SearchResult::openFailed) {
        temporaryEngine.clear();
        literalInput = true;
        literalOrigin = SearchOrigin::clipboard;
        previewInput = false;
        retrySource.clear();
        SetWindowTextW(edit, text.c_str());
        UpdateBrowserError(window, true);
    }
    RevealSearch(window);
}

bool HandleSearchCommand(HWND window, const MSG& message, unsigned modifiers)
{
    if (composing || !IsWindowVisible(window) || (message.hwnd != edit && message.hwnd != engineButton)
        || (message.message != WM_KEYDOWN && message.message != WM_SYSKEYDOWN)) return false;
    if (modifiers == shortcutAlt && message.wParam >= L'1' && message.wParam <= L'5') {
        const auto& pins = application->preferences().pinnedEngines;
        const auto index = static_cast<size_t>(message.wParam - L'1');
        if (index >= pins.size()) return false;
        if (engineFlyout) DestroyWindow(engineFlyout);
        if (suggestionFlyout) DestroyWindow(suggestionFlyout);
        SelectFlyoutEngine(window, pins[index]);
        return true;
    }
    if (message.hwnd != edit) return false;
    if (modifiers == shortcutControl && message.wParam == VK_RETURN && linkAvailable) {
        const auto result = application->openLink(ReadSearchText());
        if (result == SearchResult::opened) {
            temporaryEngine.clear();
            literalInput = false;
            previewInput = false;
            retrySource.clear();
            SetWindowTextW(edit, L"");
        } else UpdateBrowserError(window, result == SearchResult::openFailed);
        return true;
    }
    if (!modifiers && message.wParam == VK_UP && !engineFlyout && !suggestionFlyout
        && !GetWindowTextLengthW(edit) && !lastQuery.empty()) {
        literalInput = false;
        previewInput = false;
        retrySource.clear();
        SetWindowTextW(edit, lastQuery.c_str());
        SendMessageW(edit, EM_SETSEL, lastQuery.size(), static_cast<LPARAM>(lastQuery.size()));
        return true;
    }
    return false;
}

void CancelSelection(HWND window)
{
    selectionStop.request_stop();
    selectionStop = std::stop_source(std::nostopstate);
    selectionSource = nullptr;
    selectionEngine.clear();
    selectionPreview = false;
    KillTimer(window, selectionTimer);
}

void TriggerShortcut(HWND window, const EngineId& engine = {}, bool preview = false, unsigned shortcutKey = 0)
{
    YISOU_PERF_SCOPE("trigger_shortcut");
    CancelSelection(window);
    const HWND source = GetForegroundWindow();
    const auto showSearch = [&] {
        if (preview) FinishSelection(window, {}, source, engine, true);
        else if (engine.empty()) RevealSearch(window, source);
    };
    DWORD process = 0;
    GetWindowThreadProcessId(source, &process);
    if (!source || process == GetCurrentProcessId()) {
        showSearch();
        return;
    }
    // A provider that outlives the deadline must not start another worker or search stale text.
    if (selectedText.valid() && selectedText.wait_for(std::chrono::milliseconds(0)) != std::future_status::ready) {
        showSearch();
        return;
    }
    GUITHREADINFO gui{sizeof(gui)};
    GetGUIThreadInfo(GetWindowThreadProcessId(source, nullptr), &gui);
    selectionStarted = GetTickCount64();
    selectionDeadline = selectionStarted + (shortcutKey ? platform::selectionCopyTimeoutMs : 350);
    try {
        platform::SelectionCopy copy;
        if (shortcutKey) {
            selectionStop = std::stop_source{};
            copy = {window, shortcutKey, selectionDeadline, selectionStop.get_token()};
        }
        std::packaged_task<std::wstring()> read([source, focus = gui.hwndFocus, copy] {
            return platform::ReadForegroundSelection(source, focus, copy);
        });
        selectedText = read.get_future();
        std::thread(std::move(read)).detach();
    } catch (const std::system_error&) {
        CancelSelection(window);
        showSearch();
        return;
    }
    selectionSource = source;
    selectionEngine = engine;
    selectionPreview = preview;
    if (!SetTimer(window, selectionTimer, 15, nullptr)) {
        CancelSelection(window);
        showSearch();
    }
}

void PollSelection(HWND window)
{
    if (!selectionSource || GetForegroundWindow() != selectionSource) {
        CancelSelection(window);
        return;
    }
    const auto elapsed = GetTickCount64() - selectionStarted;
    if (platform::AnyShortcutModifierDown() && elapsed < 600) return;
    if (selectedText.wait_for(std::chrono::milliseconds(0)) == std::future_status::ready) {
        const HWND source = selectionSource;
        const auto engine = std::move(selectionEngine);
        const bool preview = selectionPreview;
        CancelSelection(window);
        std::wstring selection;
        try { selection = selectedText.get(); }
        catch (const std::exception&) {}
        FinishSelection(window, selection, source, engine, preview);
    } else if (GetTickCount64() >= selectionDeadline) {
        const HWND source = selectionSource;
        const auto engine = std::move(selectionEngine);
        const bool preview = selectionPreview;
        CancelSelection(window);
        FinishSelection(window, {}, source, engine, preview);
    }
}

void AddTrayIcon(HWND window)
{
    YISOU_PERF_MEMORY_SCOPE("add_tray_icon");
    NOTIFYICONDATAW icon{sizeof(icon)};
    icon.hWnd = window;
    icon.uID = trayId;
    icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    icon.uCallbackMessage = trayMessage;
    icon.hIcon = LoadIconW(nullptr, IDI_APPLICATION);
    wcscpy_s(icon.szTip, Tr(Text::appName));
    trayActive = Shell_NotifyIconW(NIM_ADD, &icon) != FALSE;
    if (trayActive) {
        icon.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &icon);
    }
}

void OpenTrayMenu(HWND window, POINT anchor)
{
    const HMENU menu = CreatePopupMenu();
    if (!menu) return;
    AppendMenuW(menu, MF_STRING, 1, Tr(Text::openSearchBox));
    AppendMenuW(menu, MF_STRING, 2, Tr(Text::openSettings));
    AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(menu, MF_STRING, 3, Tr(Text::exit));
    SetForegroundWindow(window);
    const UINT choice = TrackPopupMenuEx(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, anchor.x, anchor.y, window, nullptr);
    DestroyMenu(menu);
    PostMessageW(window, WM_NULL, 0, 0);
    if (choice == 1) RevealSearch(window);
    if (choice == 2) OpenSettings(window);
    if (choice == 3) DestroyWindow(window);
}

LRESULT CALLBACK InputProc(HWND input, UINT message, WPARAM key, LPARAM data, UINT_PTR, DWORD_PTR owner)
{
    const HWND window = reinterpret_cast<HWND>(owner);
    if (message == WM_IME_SETCONTEXT) {
        return DefWindowProcW(input, message, key, data & ~static_cast<LPARAM>(ISC_SHOWUICOMPOSITIONWINDOW));
    }
    if (message == WM_IME_STARTCOMPOSITION || message == WM_KILLFOCUS) {
        if (suggestionFlyout) DestroyWindow(suggestionFlyout);
    }
    if (message == WM_IME_STARTCOMPOSITION) composing = true;
    if (message == WM_IME_ENDCOMPOSITION || message == WM_KILLFOCUS) {
        composing = false;
        compositionText.clear();
    }
    if (message == WM_IME_COMPOSITION) {
        constexpr LPARAM resultFlags = GCS_RESULTSTR | GCS_RESULTCLAUSE | GCS_RESULTREADSTR | GCS_RESULTREADCLAUSE;
        const LRESULT result = (data & GCS_RESULTSTR)
            ? DefSubclassProc(input, message, key, data & resultFlags)
            : 0;
        UpdateCompositionText(input);
        if (!compositionText.empty()) composing = true;
        if (GetFocus() == input) RestartCaret(window);
        return result;
    }
    if (message == WM_PAINT) {
        ValidateRect(input, nullptr);
        InvalidateRect(window, nullptr, FALSE);
        return 0;
    }
    if (message == WM_ERASEBKGND) return 1;
    if (message == WM_CHAR && !composing) {
        switch (key) {
        case VK_RETURN:
            PostMessageW(window, runSearch, 0, 0);
            return 0;
        case VK_TAB:
            SetFocus(engineButton);
            return 0;
        case VK_ESCAPE:
            PostMessageW(window, WM_CLOSE, 0, 0);
            return 0;
        }
    }
    const LRESULT result = DefSubclassProc(input, message, key, data);
    switch (message) {
    case WM_SETFOCUS:
        HideCaret(input);
        RestartCaret(window);
        UpdateEngineSuggestions();
        break;
    case WM_KILLFOCUS:
        KillTimer(window, caretTimer);
        caretVisible = false;
        InvalidateRect(window, nullptr, FALSE);
        break;
    case WM_MOUSEMOVE:
        if (!(key & MK_LBUTTON)) break;
        [[fallthrough]];
    case WM_IME_ENDCOMPOSITION:
        UpdateEngineSuggestions();
        [[fallthrough]];
    case WM_LBUTTONDOWN:
    case WM_LBUTTONUP:
    case WM_LBUTTONDBLCLK:
    case WM_KEYDOWN:
    case WM_CHAR:
    case EM_SETSEL:
        if (GetFocus() == input) RestartCaret(window);
        break;
    }
    return result;
}

LRESULT CALLBACK EngineProc(HWND button, UINT message, WPARAM key, LPARAM data, UINT_PTR, DWORD_PTR owner)
{
    const HWND window = reinterpret_cast<HWND>(owner);
    switch (message) {
    case WM_MOUSEMOVE:
        if (!engineHovered) {
            engineHovered = true;
            TRACKMOUSEEVENT tracking{sizeof(tracking), TME_LEAVE, button, 0};
            TrackMouseEvent(&tracking);
            InvalidateRect(window, nullptr, FALSE);
        }
        break;
    case WM_MOUSELEAVE:
        engineHovered = false;
        InvalidateRect(window, nullptr, FALSE);
        break;
    case WM_SETFOCUS:
    case WM_KILLFOCUS:
        InvalidateRect(window, nullptr, FALSE);
        break;
    case WM_KEYDOWN:
        if (key == VK_DOWN || key == VK_UP) {
            ToggleEngineFlyout(window);
            return 0;
        }
        break;
    case WM_CHAR:
        switch (key) {
        case VK_RETURN:
            PostMessageW(window, WM_COMMAND, MAKEWPARAM(engineButtonId, BN_CLICKED), reinterpret_cast<LPARAM>(button));
            return 0;
        case VK_TAB:
        case VK_ESCAPE:
            SetFocus(edit);
            return 0;
        }
        break;
    }
    return DefSubclassProc(button, message, key, data);
}

void ApplyBackdrop(HWND window)
{
    constexpr BOOL useHostBackdrop = TRUE;
    DwmSetWindowAttribute(window, DWMWA_USE_HOSTBACKDROPBRUSH, &useHostBackdrop, sizeof(useHostBackdrop));
    backdropEnabled = SUCCEEDED(DwmSetWindowAttribute(window, systemBackdropType, &desktopAcrylic, sizeof(desktopAcrylic)));
}

LRESULT CALLBACK WindowProc(HWND window, UINT message, WPARAM key, LPARAM data)
{
#ifdef YISOU_ENABLE_PERF_TRACE
    if (perf::IsSampleMessage(message)) {
        perf::Snapshot("sample");
        return 0;
    }
#endif
    if (taskbarCreated && message == taskbarCreated) {
        AddTrayIcon(window);
        return 0;
    }
    switch (message) {
    case openExisting:
        if (key == 1 || key == 3) {
            OpenSettings(window);
            if (key == 3) ShowConfigurationSettingsPage();
        }
        else if (key == 0 || application->preferences().windowBehavior.showOnManualLaunch) RevealSearch(window);
        return 0;
    case restartConfiguration:
        PostQuitMessage(key == restartForLanguage ? restartForLanguage : restartForConfig);
        return 0;
    case WM_HOTKEY:
        YISOU_PERF_COUNT(hotkey);
        if (clipboardRegistration && clipboardRegistration->receives(key)) {
            const auto& binding = application->preferences().clipboardShortcut;
            if (binding && !CaptureRegisteredShortcut(*binding)) SearchClipboard(window, NormalizeSearchText(platform::ReadClipboardText(window)));
        }
        else if (shortcutRegistration && shortcutRegistration->receives(key)
            && !CaptureRegisteredShortcut(application->preferences().shortcut)) TriggerShortcut(window);
        else if (selectionRegistration && selectionRegistration->receives(key)) {
            const auto& binding = application->preferences().selectionShortcut;
            if (binding && !CaptureRegisteredShortcut(binding->shortcut)) {
                TriggerShortcut(window, binding->engine, binding->preview, binding->shortcut.key);
            }
        }
        return 0;
    case shortcutTriggered:
        if (!CaptureRegisteredShortcut({})) {
            const auto& excluded = application->preferences().doubleAltExcludedApps;
            if (excluded.empty() || !IsDoubleAltExcluded(SourceExecutable(GetForegroundWindow()), excluded)) TriggerShortcut(window);
        }
        return 0;
    case trayMessage:
        if (LOWORD(data) == NIN_SELECT || LOWORD(data) == NIN_KEYSELECT) {
            CancelSelection(window);
            RevealSearch(window);
        } else if (LOWORD(data) == WM_CONTEXTMENU) {
            CancelSelection(window);
            OpenTrayMenu(window, {GET_X_LPARAM(key), GET_Y_LPARAM(key)});
        }
        return 0;
    case WM_IME_SETCONTEXT:
        return DefWindowProcW(window, message, key, data & ~static_cast<LPARAM>(ISC_SHOWUICOMPOSITIONWINDOW));
    case WM_NCCALCSIZE:
        return 0;
    case WM_NCHITTEST: {
        POINT point{GET_X_LPARAM(data), GET_Y_LPARAM(data)};
        ScreenToClient(window, &point);
        return ChildWindowFromPointEx(window, point, CWP_SKIPINVISIBLE) == window ? HTCAPTION : HTCLIENT;
    }
    case WM_MOUSEACTIVATE:
        if (engineFlyout) {
            RECT button{};
            GetWindowRect(engineButton, &button);
            const DWORD position = GetMessagePos();
            if (PtInRect(&button, {GET_X_LPARAM(position), GET_Y_LPARAM(position)})) {
                DestroyWindow(engineFlyout);
                return MA_ACTIVATEANDEAT;
            }
        }
        break;
    case WM_CREATE: {
        dpi = GetDpiForWindow(window);
        const HINSTANCE instance = GetModuleHandleW(nullptr);
        edit = CreateWindowExW(0, L"EDIT", L"", WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL,
            0, 0, 0, 0, window, nullptr, instance, nullptr);
        engineButton = CreateWindowExW(0, L"BUTTON", L"", WS_CHILD | WS_VISIBLE | BS_OWNERDRAW,
            0, 0, 0, 0, window, reinterpret_cast<HMENU>(static_cast<UINT_PTR>(engineButtonId)), instance, nullptr);
        if (!edit || !engineButton
            || !SetWindowSubclass(edit, InputProc, 1, reinterpret_cast<DWORD_PTR>(window))
            || !SetWindowSubclass(engineButton, EngineProc, 1, reinterpret_cast<DWORD_PTR>(window))) {
            return -1;
        }

        UpdateEngineName();
        SendMessageW(edit, EM_LIMITTEXT, 2048, 0);
        SendMessageW(edit, EM_SETCUEBANNER, TRUE, reinterpret_cast<LPARAM>(Tr(Text::searchPlaceholder)));
        return 0;
    }
    case WM_SETFOCUS:
        SetFocus(edit);
        return 0;
    case WM_COMMAND:
        if (reinterpret_cast<HWND>(data) == engineButton && HIWORD(key) == BN_CLICKED) {
            ToggleEngineFlyout(window);
        } else if (reinterpret_cast<HWND>(data) == edit && HIWORD(key) == EN_CHANGE) {
            YISOU_PERF_SCOPE("input_change");
            UpdateSearchNotice(window, false, IsWebUrl(ReadSearchText()));
            UpdateEngineName();
            UpdateEngineSuggestions();
        }
        return 0;
    case WM_DRAWITEM: {
        const auto& item = *reinterpret_cast<const DRAWITEMSTRUCT*>(data);
        if (key == engineButtonId) {
            enginePressed = (item.itemState & ODS_SELECTED) != 0;
            InvalidateRect(window, nullptr, FALSE);
            return TRUE;
        }
        break;
    }
    case WM_CTLCOLOREDIT: {
        const auto dc = reinterpret_cast<HDC>(key);
        SetBkColor(dc, RGB(255, 255, 255));
        SetTextColor(dc, RGB(26, 32, 41));
        return reinterpret_cast<LRESULT>(GetStockObject(WHITE_BRUSH));
    }
    case runSearch:
        Search(window);
        return 0;
    case WM_DPICHANGED: {
        if (engineFlyout) DestroyWindow(engineFlyout);
        if (suggestionFlyout) DestroyWindow(suggestionFlyout);
        dpi = LOWORD(key);
        engineBitmap.Reset();
        for (auto& bitmap : chevronBitmaps) bitmap.Reset();
        const auto* bounds = reinterpret_cast<const RECT*>(data);
        SetWindowPos(window, nullptr, bounds->left, bounds->top, Scale(width), Scale(WindowHeight()),
            SWP_NOACTIVATE | SWP_NOZORDER);
        if (engineFormat) {
            SetEditFont(CreateUiFont(uiFontTemplate, inputFontSize));
            LayoutControls();
        }
        InvalidateRect(window, nullptr, FALSE);
        return 0;
    }
    case WM_EXITSIZEMOVE: {
        RECT bounds{};
        MonitorInfo info;
        if (GetWindowRect(window, &bounds) && ReadMonitor(TargetMonitor(window), info)) {
            platform::SaveDisplayPosition(info.szDevice, {
                MulDiv(bounds.left - info.rcWork.left, USER_DEFAULT_SCREEN_DPI, dpi),
                MulDiv(bounds.top - info.rcWork.top, USER_DEFAULT_SCREEN_DPI, dpi)});
        }
        return 0;
    }
    case WM_MOVE:
        if (suggestionFlyout) RepositionEngineSuggestions(suggestionFlyout);
        break;
    case WM_DISPLAYCHANGE:
        if (engineFlyout) DestroyWindow(engineFlyout);
        if (suggestionFlyout) DestroyWindow(suggestionFlyout);
        PositionSearch(window, TargetMonitor(window));
        return 0;
    case WM_SETTINGCHANGE:
        if (key != SPI_SETWORKAREA) break;
        PositionSearch(window, TargetMonitor(window));
        return 0;
    case WM_DWMCOMPOSITIONCHANGED:
        ApplyBackdrop(window);
        RedrawWindow(window, nullptr, nullptr, RDW_INVALIDATE | RDW_ALLCHILDREN);
        return 0;
    case WM_TIMER:
        if (key == graphicsIdleTimer) {
            YISOU_PERF_COUNT(graphicsTimer);
            KillTimer(window, graphicsIdleTimer);
            if (!IsWindowVisible(window) && !HasOpenSettings() && !engineFlyout && !suggestionFlyout) {
                ReleaseComposition();
            } else {
                if (drawingDevice) drawingDevice->ClearResources(0);
                if (dxgiDevice) dxgiDevice->Trim();
            }
            return 0;
        }
        if (key == selectionTimer) {
            YISOU_PERF_COUNT(selectionTimer);
            PollSelection(window);
            return 0;
        }
        if (key == caretTimer) {
            YISOU_PERF_COUNT(caretTimer);
            if (!caretVisual || GetFocus() != edit) return 0;
            caretVisible = !caretVisible;
            if (FAILED(caretVisual->SetVisible(caretVisible)) || FAILED(composition->Commit())) {
                InvalidateRect(window, nullptr, FALSE);
            }
            return 0;
        }
        break;
    case WM_PAINT:
        ValidateRect(window, nullptr);
        if (composition && IsWindowVisible(window)) {
            const HRESULT result = Paint(window);
            if (result == D2DERR_RECREATE_TARGET || result == DXGI_ERROR_DEVICE_REMOVED
                || result == DXGI_ERROR_DEVICE_RESET) {
                ReleaseComposition();
                if (SUCCEEDED(InitComposition(window))) Paint(window);
            }
        }
        return 0;
    case WM_CLOSE:
        CancelSelection(window);
        if (engineFlyout) DestroyWindow(engineFlyout);
        if (suggestionFlyout) DestroyWindow(suggestionFlyout);
        if (previewInput) {
            temporaryEngine.clear();
            literalInput = false;
            previewInput = false;
            retrySource.clear();
            SetWindowTextW(edit, L"");
        }
        if (!literalInput && (!temporaryEngine.empty() || application->parseInput(ReadSearchText()).usesAlias)) {
            const auto value = ReadSearchText();
            const auto input = application->parseInput(value);
            temporaryEngine.clear();
            if (input.usesAlias) {
                const std::wstring query(input.query);
                SetWindowTextW(edit, query.c_str());
            }
            UpdateEngineName();
        }
        if (application->preferences().windowBehavior.closeToTray && (trayActive || shortcutActive || selectionShortcutActive
            || clipboardShortcutActive)) {
            ShowWindow(window, SW_HIDE);
            ScheduleGraphicsCleanup(window);
        } else DestroyWindow(window);
        return 0;
    case WM_ENDSESSION:
        if (key) DestroyWindow(window);
        return 0;
    case WM_DESTROY: {
        KillTimer(window, graphicsIdleTimer);
        CancelSelection(window);
        if (shortcutRegistration) shortcutRegistration->clear();
        if (selectionRegistration) selectionRegistration->clear();
        if (clipboardRegistration) clipboardRegistration->clear();
        if (trayActive) {
            NOTIFYICONDATAW icon{sizeof(icon)};
            icon.hWnd = window;
            icon.uID = trayId;
            Shell_NotifyIconW(NIM_DELETE, &icon);
            trayActive = false;
        }
        if (engineFlyout) DestroyWindow(engineFlyout);
        if (suggestionFlyout) DestroyWindow(suggestionFlyout);
        PostQuitMessage(0);
        return 0;
    }
    case WM_NCDESTROY:
        if (editFont) DeleteObject(editFont);
        editFont = nullptr;
        edit = nullptr;
        engineButton = nullptr;
        displayedEngineName.clear();
        ReleaseComposition();
        break;
    }
    return DefWindowProcW(window, message, key, data);
}

HWND CreateSearchWindow(HINSTANCE instance)
{
    YISOU_PERF_MEMORY_SCOPE("create_search_window");
    WNDCLASSW windowClass{};
    windowClass.hInstance = instance;
    windowClass.lpszClassName = searchClassName;
    windowClass.lpfnWndProc = WindowProc;
    windowClass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    if (!RegisterClassW(&windowClass) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) return nullptr;

    MONITORINFO monitor{sizeof(monitor)};
    const auto target = TargetMonitor(GetForegroundWindow());
    if (!GetMonitorInfoW(target, &monitor)) return nullptr;
    const auto work = monitor.rcWork;
    HWND window = CreateWindowExW(WS_EX_NOREDIRECTIONBITMAP, windowClass.lpszClassName, Tr(Text::appName),
        WS_POPUP | WS_THICKFRAME, work.left, work.top, width, height,
        nullptr, nullptr, instance, nullptr);
    if (!window) return nullptr;

    const DWM_WINDOW_CORNER_PREFERENCE corners = DWMWCP_ROUND;
    DwmSetWindowAttribute(window, DWMWA_WINDOW_CORNER_PREFERENCE, &corners, sizeof(corners));
    constexpr COLORREF border = 0xfffffffe; // DWMWA_COLOR_NONE
    DwmSetWindowAttribute(window, DWMWA_BORDER_COLOR, &border, sizeof(border));
    ApplyBackdrop(window);
    SetWindowPos(window, nullptr, 0, 0, Scale(width), Scale(height),
        SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE);
    PositionSearch(window, target);
    return window;
}

int RunApplication(HINSTANCE instance, LaunchMode mode)
{
    const HWND window = CreateSearchWindow(instance);
    if (!window) return 1;
    platform::ShortcutRegistration registration(window, shortcutTriggered);
    platform::ShortcutRegistration selection(window, 0, 3);
    platform::ShortcutRegistration clipboard(window, 0, 5);
    shortcutRegistration = &registration;
    selectionRegistration = &selection;
    clipboardRegistration = &clipboard;
    shortcutActive = registration.apply(application->preferences().shortcut) == platform::ShortcutResult::applied;
    if (const auto& binding = application->preferences().selectionShortcut) {
        selectionShortcutActive = selection.apply(binding->shortcut) == platform::ShortcutResult::applied;
    }
    if (const auto& binding = application->preferences().clipboardShortcut) {
        clipboardShortcutActive = clipboard.apply(*binding) == platform::ShortcutResult::applied;
    }
    taskbarCreated = RegisterWindowMessageW(L"TaskbarCreated");
    AddTrayIcon(window);
    if (mode == LaunchMode::settings || mode == LaunchMode::configuration) {
        OpenSettings(window);
        if (mode == LaunchMode::configuration) ShowConfigurationSettingsPage();
    }
    else if ((mode == LaunchMode::manual && application->preferences().windowBehavior.showOnManualLaunch)
        || (!trayActive && !shortcutActive)) RevealSearch(window);
    YISOU_PERF_SNAPSHOT("ready");

    MSG message{};
    BOOL received;
    while ((received = GetMessageW(&message, nullptr, 0, 0)) > 0) {
        YISOU_PERF_COUNT(queuedMessage);
        if ((message.message == WM_KEYDOWN || message.message == WM_SYSKEYDOWN)
            && (message.hwnd == edit || message.hwnd == engineButton)
            && HandleSearchCommand(window, message, platform::ShortcutModifiers())) continue;
        if (HandleSearchSuggestionKey(message)) continue;
        if (HandleEngineFlyoutMessage(engineFlyout, message)) {
            if (message.wParam == VK_ESCAPE && GetForegroundWindow() == window) SetFocus(edit);
            continue;
        }
        if (HandleSettingsMessage(message)) continue;
        if (!composing && message.message == WM_KEYDOWN && message.wParam == VK_OEM_COMMA
            && (GetKeyState(VK_CONTROL) & 0x8000)
            && (message.hwnd == window || IsChild(window, message.hwnd))) {
            OpenSettings(window);
            continue;
        }
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    if (IsWindow(window)) DestroyWindow(window);
    shortcutRegistration = nullptr;
    selectionRegistration = nullptr;
    clipboardRegistration = nullptr;
    shortcutActive = false;
    selectionShortcutActive = false;
    clipboardShortcutActive = false;
    return received == -1 ? 1 : static_cast<int>(message.wParam);
}

bool InitializeSearchResources(Application& app)
{
    YISOU_PERF_MEMORY_SCOPE("initialize_com");
    application = &app;
    const auto com = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    uninitializeCom = SUCCEEDED(com);
    return uninitializeCom || com == RPC_E_CHANGED_MODE;
}

void ReleaseSearchResources()
{
    engineFormat.Reset();
    if (activeFont.source == FontSource::file) {
        RemoveFontResourceExW(activeFont.value.c_str(), privateFontFlags, nullptr);
    }
    activeFont = {};
    write.Reset();
    drawingFactory.Reset();
    if (uninitializeCom) CoUninitialize();
    uninitializeCom = false;
    application = nullptr;
}

}

void ActivateSearchWindow(LaunchMode mode)
{
    if (mode == LaunchMode::startup) return;
    const HWND existing = FindWindowW(searchClassName, nullptr);
    if (existing) {
        DWORD id = 0;
        GetWindowThreadProcessId(existing, &id);
        AllowSetForegroundWindow(id);
        PostMessageW(existing, openExisting, mode == LaunchMode::configuration ? 3 : mode == LaunchMode::settings ? 1 : 2, 0);
    }
}

int RunSearchWindow(HINSTANCE instance, Application& app, LaunchMode mode)
{
    const int result = InitializeSearchResources(app) ? RunApplication(instance, mode) : 1;
    ReleaseSearchResources();
    return result;
}

}
