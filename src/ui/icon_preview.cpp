#include "icon_preview.h"

#include "engine_icons.h"

#include <algorithm>
#include <array>
#include <cstdlib>
#include <cstring>
#include <format>
#include <d2d1_3.h>
#include <d3d11.h>
#include <dwrite.h>
#include <shlwapi.h>
#include <wincodec.h>
#include <wrl/client.h>

namespace yisou::ui {
namespace {

using Microsoft::WRL::ComPtr;

constexpr wchar_t windowClass[] = L"YiSouIconPreview";
constexpr wchar_t windowTitle[] = L"易搜 · 图标对比";
constexpr DWORD windowStyle = WS_OVERLAPPEDWINDOW | WS_HSCROLL | WS_VSCROLL;
constexpr std::array sizes{16, 18, 20, 24, 30, 36, 42, 48, 64, 72};
constexpr std::array largeSizes{128, 256, 512, 1024};
constexpr std::array methods{L"Direct2D SVG", L"小尺寸专版 SVG", L"原尺寸 PNG", L"PNG · 高质量三次", L"PathGeometry", L"DirectWrite"};
enum Method { svg, smallSvg, png, cubic, geometry, letter };
constexpr std::array algorithmMethods{L"Direct2D SVG", L"Blend2D", L"Manson–Schaefer",
    L"联合采样 16×16", L"FreeType AutoHint", L"Windfoil WGSL", L"Slug · WGSL 移植"};
constexpr std::array productionMethods{L"Direct2D SVG", L"resvg 16×", L"Cairo 64× 参考"};
enum class PreviewMode { production, algorithms, formats };
enum class SmallArtwork { none, search, flame, youtubeMono, youtubeColor };

struct Sample {
    const wchar_t* resource;
    const wchar_t* label;
    wchar_t glyph{};
    bool outline{};
    SmallArtwork compactArtwork{};
};

constexpr std::array samples{
    Sample{L"preview_search", L"搜索 · Tabler 单色描边", 0, true, SmallArtwork::search},
    Sample{L"preview_yandex", L"Yandex · 单色", L'Y'},
    Sample{L"preview_youtube_mono", L"YouTube · 单色", 0, false, SmallArtwork::youtubeMono},
    Sample{L"preview_youtube_color", L"YouTube · 多色", 0, false, SmallArtwork::youtubeColor},
    Sample{L"preview_google", L"Google · 多色", L'G'},
    Sample{L"preview_bilibili", L"哔哩哔哩 · 品牌色"},
    Sample{L"preview_flame", L"火焰 · Tabler 不规则曲线", 0, true, SmallArtwork::flame},
};
constexpr size_t initialSample = samples.size() - 1;
constexpr int padding = 16;
constexpr int labelWidth = 63;
constexpr int columnWidth = 129;
constexpr int toolbarHeight = 47;
constexpr int headerHeight = 79;
constexpr int clientWidth = 2 * padding + labelWidth + columnWidth * static_cast<int>(algorithmMethods.size());
constexpr int sampleId = 1;
constexpr int sizeId = 2;
constexpr int fullscreenId = 3;
constexpr int modeId = 4;

std::string_view Resource(const wchar_t* name)
{
    const HMODULE module = GetModuleHandleW(nullptr);
    const HRSRC resource = FindResourceW(module, name, RT_RCDATA);
    if (!resource) return {};
    const auto* bytes = static_cast<const char*>(LockResource(LoadResource(module, resource)));
    return bytes ? std::string_view(bytes, SizeofResource(module, resource)) : std::string_view{};
}

ComPtr<IStream> ResourceStream(const wchar_t* name)
{
    const auto data = Resource(name);
    ComPtr<IStream> stream;
    if (!data.empty()) {
        stream.Attach(SHCreateMemStream(reinterpret_cast<const BYTE*>(data.data()), static_cast<UINT>(data.size())));
    }
    return stream;
}

std::string SmallSvg(SmallArtwork artwork, int size)
{
    if (size > 24 || artwork == SmallArtwork::none) return {};
    constexpr auto square = R"(<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {0} {0}">{1}</svg>)";
    switch (artwork) {
    case SmallArtwork::search: {
        const int left = 2;
        const int right = MulDiv(size, 18, 24);
        const float center = (left + right) / 2.f;
        const float radius = (right - left - 2) / 2.f;
        const float start = center + radius * 0.70710678f;
        const int end = size - 2;
        const auto outline = std::format(
            R"(<g fill="none" stroke="black" stroke-width="2" stroke-linecap="round"><circle cx="{0}" cy="{0}" r="{1}"/><path d="M{2} {2} L{3} {3}"/></g>)",
            center, radius, start, end);
        return std::format(square, size, outline);
    }
    case SmallArtwork::flame: {
        // Give the inner notch more space and use a single broad curve for the base.
        const float stroke = size < 20 ? 2.25f : 2.f;
        const auto outline = std::format(
            R"(<path d="M11 3 C11 7 5 9 5 14 C5 18 8 21 12 21 C16 21 19 18 19 14 C19 12 18 10 16 8 C16 12 13 13 11 12 C13 9 13 6 11 3 Z" fill="none" stroke="black" stroke-width="{}" stroke-linejoin="round"/>)",
            stroke);
        return std::format(square, 24, outline);
    }
    case SmallArtwork::youtubeMono:
    case SmallArtwork::youtubeColor: {
        const int margin = (size - MulDiv(size, 3, 4)) / 2;
        const int top = margin;
        const int bottom = size - margin;
        const int left = size / 2 - MulDiv(size, 1, 8);
        const int tip = size / 2 + MulDiv(size, 1, 5);
        const int center = size / 2;
        const int half = MulDiv(size, 1, 5);
        const auto triangle = std::format("M{} {} L{} {} L{} {} Z", left, center - half,
            tip, center, left, center + half);
        const int corner = MulDiv(size, 1, 5);
        if (artwork == SmallArtwork::youtubeColor) {
            const auto logo = std::format(
                R"(<rect x="0" y="{}" width="{}" height="{}" rx="{}" fill="red"/><path d="{}" fill="white"/>)",
                top, size, bottom - top, corner, triangle);
            return std::format(square, size, logo);
        }
        // Even-odd fill leaves the play triangle transparent on any background.
        const int right = size - corner;
        const auto logo = std::format(
            R"(<path fill-rule="evenodd" d="M{0} {1} H{2} Q{3} {1} {3} {4} V{5} Q{3} {6} {2} {6} H{0} Q0 {6} 0 {5} V{4} Q0 {1} {0} {1} Z {7}"/>)",
            corner, top, right, size, top + corner, bottom - corner, bottom, triangle);
        return std::format(square, size, logo);
    }
    case SmallArtwork::none:
        return {};
    }
    return {};
}

ComPtr<IWICBitmapSource> OpenPng(IWICImagingFactory& factory, const wchar_t* name)
{
    const auto stream = ResourceStream(name);
    ComPtr<IWICBitmapDecoder> decoder;
    ComPtr<IWICBitmapFrameDecode> frame;
    ComPtr<IWICFormatConverter> converter;
    if (!stream || FAILED(factory.CreateDecoderFromStream(stream.Get(), nullptr,
            WICDecodeMetadataCacheOnLoad, decoder.GetAddressOf()))
        || FAILED(decoder->GetFrame(0, frame.GetAddressOf()))
        || FAILED(factory.CreateFormatConverter(converter.GetAddressOf()))
        || FAILED(converter->Initialize(frame.Get(), GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone, nullptr, 0., WICBitmapPaletteTypeCustom))) return {};

    ComPtr<IWICBitmapSource> source = converter;
    return source;
}

IconPixels CopyPng(IWICBitmapSource& source, size_t sample, size_t row, int edge)
{
    const WICRect bounds{static_cast<int>(sample) * edge, static_cast<int>(row) * edge, edge, edge};
    IconPixels pixels(static_cast<size_t>(edge) * edge);
    if (FAILED(source.CopyPixels(&bounds, edge * sizeof(std::uint32_t), static_cast<UINT>(pixels.size() * sizeof(std::uint32_t)),
        reinterpret_cast<BYTE*>(pixels.data())))) return {};
    return pixels;
}

IconPixels PngPixels(IWICImagingFactory& factory, size_t sample, int targetSize, bool large)
{
    const auto name = (large ? L"preview_large_" : L"preview_native_") + std::to_wstring(targetSize);
    const auto source = OpenPng(factory, name.c_str());
    return source ? CopyPng(*source.Get(), sample, 0, large ? 256 + targetSize : targetSize) : IconPixels{};
}

constexpr D2D1_PIXEL_FORMAT pixelFormat{DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED};

template<class Draw>
IconPixels Rasterize(ID2D1DeviceContext& canvas, int size, Draw draw)
{
    const auto dimensions = D2D1::SizeU(size, size);
    const auto targetProperties = D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_TARGET, pixelFormat, 96, 96);
    const auto copyProperties = D2D1::BitmapProperties1(
        D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW, pixelFormat, 96, 96);
    ComPtr<ID2D1Bitmap1> target, copy;
    if (FAILED(canvas.CreateBitmap(dimensions, nullptr, 0, &targetProperties, target.GetAddressOf()))
        || FAILED(canvas.CreateBitmap(dimensions, nullptr, 0, &copyProperties, copy.GetAddressOf()))) return {};

    canvas.SetTarget(target.Get());
    canvas.SetTransform(D2D1::Matrix3x2F::Identity());
    canvas.BeginDraw();
    canvas.Clear(D2D1::ColorF(0, 0.f));
    draw();
    const auto result = canvas.EndDraw();
    canvas.SetTarget(nullptr);
    D2D1_MAPPED_RECT mapped{};
    if (FAILED(result) || FAILED(copy->CopyFromBitmap(nullptr, target.Get(), nullptr))
        || FAILED(copy->Map(D2D1_MAP_OPTIONS_READ, &mapped))) return {};

    IconPixels pixels(static_cast<size_t>(size) * size);
    for (int row = 0; row < size; ++row) {
        std::memcpy(pixels.data() + static_cast<size_t>(row) * size,
            mapped.bits + static_cast<size_t>(row) * mapped.pitch, size * sizeof(std::uint32_t));
    }
    copy->Unmap();
    return pixels;
}

IconPixels CubicPixels(ID2D1DeviceContext& canvas, const IconPixels& pixels, int size)
{
    const int edge = 256 + size;
    const auto properties = D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_NONE, pixelFormat, 96, 96);
    ComPtr<ID2D1Bitmap1> source;
    if (pixels.empty() || FAILED(canvas.CreateBitmap(D2D1::SizeU(edge, edge),
        pixels.data(), edge * sizeof(std::uint32_t), &properties, source.GetAddressOf()))) return {};
    const float extent = static_cast<float>(size);
    return Rasterize(canvas, size, [&] {
        const auto bounds = D2D1::RectF(0, 0, extent, extent);
        canvas.DrawBitmap(source.Get(), &bounds, 1.f, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, nullptr, nullptr);
    });
}

IconPixels SvgPixels(ID2D1DeviceContext5& canvas, const Sample& sample, int size)
{
    const auto stream = ResourceStream(sample.resource);
    const float edge = static_cast<float>(size);
    ComPtr<ID2D1SvgDocument> document;
    if (!stream || FAILED(canvas.CreateSvgDocument(stream.Get(), D2D1::SizeF(edge, edge), document.GetAddressOf()))) return {};
    ComPtr<ID2D1SvgElement> root;
    document->GetRoot(root.GetAddressOf());
    const D2D1_SVG_LENGTH extent{edge, D2D1_SVG_LENGTH_UNITS_NUMBER};
    if (!root || FAILED(root->SetAttributeValue(L"width", extent))
        || FAILED(root->SetAttributeValue(L"height", extent))) return {};
    return Rasterize(canvas, size, [&] { canvas.DrawSvgDocument(document.Get()); });
}

struct GeometryLayer {
    ComPtr<ID2D1PathGeometry1> path;
    D2D1_COLOR_F color{};
};

struct SvgGeometry {
    D2D1_SVG_VIEWBOX viewBox{};
    std::vector<GeometryLayer> layers;
    ComPtr<ID2D1StrokeStyle> stroke;
};

SvgGeometry ReadGeometry(ID2D1DeviceContext5& canvas, const Sample& sample)
{
    SvgGeometry icon;
    const auto stream = ResourceStream(sample.resource);
    ComPtr<ID2D1SvgDocument> document;
    if (!stream || FAILED(canvas.CreateSvgDocument(stream.Get(), D2D1::SizeF(24, 24), document.GetAddressOf()))) return {};
    ComPtr<ID2D1SvgElement> root;
    document->GetRoot(root.GetAddressOf());
    if (!root || FAILED(root->GetAttributeValue(L"viewBox", D2D1_SVG_ATTRIBUTE_POD_TYPE_VIEWBOX,
        &icon.viewBox, sizeof(icon.viewBox)))) return {};

    ComPtr<ID2D1SvgElement> child;
    root->GetFirstChild(child.GetAddressOf());
    while (child) {
        ComPtr<ID2D1SvgPathData> data;
        if (SUCCEEDED(child->GetAttributeValue(L"d", data.GetAddressOf())) && data) {
            auto rule = D2D1_FILL_MODE_WINDING;
            if (root->IsAttributeSpecified(L"fill-rule")) root->GetAttributeValue(L"fill-rule", &rule);
            if (child->IsAttributeSpecified(L"fill-rule")) child->GetAttributeValue(L"fill-rule", &rule);
            GeometryLayer layer;
            layer.color = D2D1::ColorF(D2D1::ColorF::Black);
            ComPtr<ID2D1SvgPaint> paint;
            if (child->IsAttributeSpecified(L"fill") && SUCCEEDED(child->GetAttributeValue(L"fill", paint.GetAddressOf()))
                && paint && paint->GetPaintType() == D2D1_SVG_PAINT_TYPE_COLOR) paint->GetColor(&layer.color);
            if (FAILED(data->CreatePathGeometry(rule, layer.path.GetAddressOf()))) return {};
            icon.layers.push_back(std::move(layer));
        }
        ComPtr<ID2D1SvgElement> next;
        root->GetNextChild(child.Get(), next.GetAddressOf());
        child = std::move(next);
    }

    if (sample.outline) {
        ComPtr<ID2D1Factory> factory;
        canvas.GetFactory(factory.GetAddressOf());
        const auto style = D2D1::StrokeStyleProperties(D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_ROUND,
            D2D1_CAP_STYLE_ROUND, D2D1_LINE_JOIN_ROUND);
        factory->CreateStrokeStyle(style, nullptr, 0, icon.stroke.GetAddressOf());
    }
    return icon;
}

IconPixels GeometryPixels(ID2D1DeviceContext& canvas, const SvgGeometry& icon, int size)
{
    if (icon.layers.empty() || icon.viewBox.width <= 0 || icon.viewBox.height <= 0) return {};
    const auto& view = icon.viewBox;
    const float edge = static_cast<float>(size);
    const float scale = edge / std::max(view.width, view.height);
    ComPtr<ID2D1SolidColorBrush> brush;
    if (FAILED(canvas.CreateSolidColorBrush(D2D1::ColorF(0u), brush.GetAddressOf()))) return {};
    return Rasterize(canvas, size, [&] {
        canvas.SetTransform(D2D1::Matrix3x2F::Scale(scale, scale) * D2D1::Matrix3x2F::Translation(
            (edge - view.width * scale) / 2 - view.x * scale, (edge - view.height * scale) / 2 - view.y * scale));
        for (const auto& layer : icon.layers) {
            brush->SetColor(layer.color);
            if (icon.stroke) canvas.DrawGeometry(layer.path.Get(), brush.Get(), 2.f, icon.stroke.Get());
            else canvas.FillGeometry(layer.path.Get(), brush.Get());
        }
    });
}

IconPixels LetterPixels(ID2D1DeviceContext& canvas, IDWriteFactory& factory, wchar_t glyph, int size)
{
    if (!glyph) return {};
    const float extent = static_cast<float>(size);
    ComPtr<IDWriteTextFormat> format;
    ComPtr<ID2D1SolidColorBrush> brush;
    if (FAILED(factory.CreateTextFormat(L"Segoe UI", nullptr, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL, extent * .92f, L"en-US", format.GetAddressOf()))
        || FAILED(canvas.CreateSolidColorBrush(D2D1::ColorF(0u), brush.GetAddressOf()))) return {};
    format->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
    format->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    return Rasterize(canvas, size, [&] {
        canvas.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        canvas.DrawText(&glyph, 1, format.Get(), D2D1::RectF(0, 0, extent, extent), brush.Get());
    });
}

struct Preview {
    ComPtr<ID2D1DeviceContext5> canvas;
    ComPtr<IWICImagingFactory> wic;
    ComPtr<IDWriteFactory> write;
    std::array<std::array<IconPixels, algorithmMethods.size()>, sizes.size()> pixels;
    HFONT font{};
    HWND combo{};
    HWND sizeCombo{};
    HWND modeCombo{};
    HWND fullscreenButton{};
    WINDOWPLACEMENT windowed{sizeof(WINDOWPLACEMENT)};
    bool fullscreen{};
    PreviewMode mode{PreviewMode::production};
    UINT dpi{USER_DEFAULT_SCREEN_DPI};
    size_t sample{initialSample};
    int largeSize{};

    ~Preview() { if (font) DeleteObject(font); }

    int Px(int dip) const { return MulDiv(dip, dpi, USER_DEFAULT_SCREEN_DPI); }
    int RowHeight(int size) const { return std::max(size + 6, Px(30)); }
    std::span<const int> VisibleSizes() const
    {
        return largeSize ? std::span<const int>(&largeSize, 1) : std::span<const int>(sizes);
    }
    std::span<const wchar_t* const> VisibleMethods() const
    {
        if (mode == PreviewMode::production) return productionMethods;
        if (mode == PreviewMode::algorithms) return algorithmMethods;
        return methods;
    }
    int ColumnWidth() const { return std::max(Px(columnWidth), largeSize + Px(16)); }

    void Select(size_t index)
    {
        sample = index;
        const auto artwork = Resource(samples[index].resource);
        const auto paths = mode == PreviewMode::formats ? ReadGeometry(*canvas.Get(), samples[index]) : SvgGeometry{};
        const auto visible = VisibleSizes();
        for (size_t row = 0; row < visible.size(); ++row) {
            const int size = visible[row];
            auto& images = pixels[row];
            images[svg] = SvgPixels(*canvas.Get(), samples[index], size);
            if (mode == PreviewMode::production) {
                images[1] = RasterizeSvgIcon(artwork, size);
                const auto name = L"preview_reference_" + std::to_wstring(size);
                const auto sheet = OpenPng(*wic.Get(), name.c_str());
                images[2] = sheet ? CopyPng(*sheet.Get(), index, 0, size) : IconPixels{};
                continue;
            }
            if (mode == PreviewMode::algorithms) {
                const auto name = L"preview_algorithms_" + std::to_wstring(size);
                const auto sheet = OpenPng(*wic.Get(), name.c_str());
                for (size_t col = 1; col < algorithmMethods.size(); ++col) {
                    images[col] = sheet ? CopyPng(*sheet.Get(), index, col - 1, size) : IconPixels{};
                }
                continue;
            }
            images[smallSvg].clear();
            if (const auto compact = SmallSvg(samples[index].compactArtwork, size); !compact.empty()) {
                images[smallSvg] = RasterizeSvgIcon(compact, size);
            }
            images[png] = PngPixels(*wic.Get(), index, size, false);
            images[cubic] = CubicPixels(*canvas.Get(), PngPixels(*wic.Get(), index, size, true), size);
            images[geometry] = GeometryPixels(*canvas.Get(), paths, size);
            images[letter] = LetterPixels(*canvas.Get(), *write.Get(), samples[index].glyph, size);
        }
    }
};

void ScrollRange(HWND window, int bar, int extent, int page, bool reset)
{
    SCROLLINFO scroll{sizeof(scroll), SIF_RANGE | SIF_PAGE};
    scroll.nMax = std::max(0, extent - 1);
    scroll.nPage = static_cast<UINT>(std::max(0, page));
    if (reset) scroll.fMask |= SIF_POS;
    SetScrollInfo(window, bar, &scroll, TRUE);
}

void UpdateScrollbars(HWND window, const Preview& preview, bool reset = false)
{
    const int width = preview.Px(padding * 2 + labelWidth) + preview.ColumnWidth() * static_cast<int>(preview.VisibleMethods().size());
    int height = preview.Px(headerHeight - toolbarHeight + padding);
    for (const int size : preview.VisibleSizes()) height += preview.RowHeight(size);
    RECT bounds{};
    for (int pass = 0; pass < 2; ++pass) {
        GetClientRect(window, &bounds);
        ScrollRange(window, SB_HORZ, width, bounds.right, reset);
        GetClientRect(window, &bounds);
        ScrollRange(window, SB_VERT, height, bounds.bottom - preview.Px(toolbarHeight), reset);
    }
    InvalidateRect(window, nullptr, FALSE);
}

void Scroll(HWND window, int bar, int command, int step)
{
    SCROLLINFO info{sizeof(info), SIF_ALL};
    if (!GetScrollInfo(window, bar, &info)) return;
    switch (command) {
    case SB_LINEUP: info.nPos -= step; break;
    case SB_LINEDOWN: info.nPos += step; break;
    case SB_PAGEUP: info.nPos -= static_cast<int>(info.nPage); break;
    case SB_PAGEDOWN: info.nPos += static_cast<int>(info.nPage); break;
    case SB_THUMBTRACK:
    case SB_THUMBPOSITION: info.nPos = info.nTrackPos; break;
    default: return;
    }
    info.fMask = SIF_POS;
    SetScrollInfo(window, bar, &info, TRUE);
    InvalidateRect(window, nullptr, FALSE);
}

void Fill(HDC dc, const RECT& bounds, COLORREF color)
{
    SetDCBrushColor(dc, color);
    FillRect(dc, &bounds, static_cast<HBRUSH>(GetStockObject(DC_BRUSH)));
}

void Label(HDC dc, std::wstring_view text, RECT bounds, UINT alignment = DT_CENTER)
{
    DrawTextW(dc, text.data(), static_cast<int>(text.size()), &bounds,
        alignment | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
}

void Paint(HWND window, const Preview& preview)
{
    PAINTSTRUCT paint{};
    const HDC dc = BeginPaint(window, &paint);
    Fill(dc, paint.rcPaint, RGB(245, 247, 250));
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, RGB(27, 39, 49));
    const auto previousFont = SelectObject(dc, preview.font);
    const int toolbar = preview.Px(toolbarHeight);
    const int column = preview.ColumnWidth();
    const int offset = GetScrollPos(window, SB_HORZ);
    const int gridLeft = preview.Px(padding + labelWidth) - offset;
    const auto headings = preview.VisibleMethods();
    const int gridRight = gridLeft + column * static_cast<int>(headings.size());
    const int headerTop = toolbar - GetScrollPos(window, SB_VERT);
    const int headerBottom = headerTop + preview.Px(headerHeight - toolbarHeight);
    constexpr auto border = RGB(221, 227, 234);

    Label(dc, L"样本", {preview.Px(padding + 4), preview.Px(10), preview.Px(padding + labelWidth), preview.Px(39)}, DT_LEFT);
    const int saved = SaveDC(dc);
    RECT client{};
    GetClientRect(window, &client);
    IntersectClipRect(dc, 0, toolbar, client.right, client.bottom);
    const int gridStart = preview.Px(padding) - offset;
    Label(dc, L"尺寸", {gridStart, headerTop, gridLeft, headerBottom});
    for (size_t col = 0; col < headings.size(); ++col) {
        const int x = gridLeft + static_cast<int>(col) * column;
        Label(dc, headings[col], {x, headerTop, x + column, headerBottom});
    }

    int top = headerBottom;
    const auto visible = preview.VisibleSizes();
    for (size_t row = 0; row < visible.size(); ++row) {
        const int size = visible[row];
        const int bottom = top + preview.RowHeight(size);
        if (bottom <= paint.rcPaint.top || top >= paint.rcPaint.bottom) {
            top = bottom;
            continue;
        }
        Fill(dc, {gridStart, top, gridRight, bottom}, RGB(255, 255, 255));
        Fill(dc, {gridStart, top, gridRight, top + 1}, border);
        Label(dc, std::to_wstring(size) + L" px", {gridStart, top, gridLeft, bottom});
        for (size_t col = 0; col < headings.size(); ++col) {
            const int split = gridLeft + static_cast<int>(col) * column;
            if (split + column <= paint.rcPaint.left || split >= paint.rcPaint.right) continue;
            Fill(dc, {split, top, split + 1, bottom}, border);
            const int left = split + (column - size) / 2;
            const int y = top + (bottom - top - size) / 2;
            const auto& pixels = preview.pixels[row][col];
            if (pixels.empty()) {
                const auto& sample = samples[preview.sample];
                const bool absent = preview.mode == PreviewMode::formats && ((col == letter && !sample.glyph)
                    || (col == smallSvg && (size > 24 || sample.compactArtwork == SmallArtwork::none)));
                Label(dc, absent ? L"—" : L"不可用",
                    {split, top, split + column, bottom});
            } else {
                DrawIconPixels(dc, pixels, {left, y, left + size, y + size}, size);
            }
        }
        top = bottom;
    }
    Fill(dc, {gridStart, top, gridRight, top + 1}, border);
    RestoreDC(dc, saved);
    SelectObject(dc, previousFont);
    EndPaint(window, &paint);
}

void Layout(HWND window, Preview& preview)
{
    preview.dpi = GetDpiForWindow(window);
    const auto font = CreateFontW(-preview.Px(13), 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE, DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH, L"Segoe UI");
    if (font) {
        SendMessageW(preview.combo, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
        SendMessageW(preview.sizeCombo, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
        SendMessageW(preview.modeCombo, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
        SendMessageW(preview.fullscreenButton, WM_SETFONT, reinterpret_cast<WPARAM>(font), TRUE);
        if (preview.font) DeleteObject(preview.font);
        preview.font = font;
    }
    MoveWindow(preview.combo, preview.Px(padding + labelWidth), preview.Px(10), preview.Px(238), preview.Px(260), TRUE);
    MoveWindow(preview.sizeCombo, preview.Px(327), preview.Px(10), preview.Px(115), preview.Px(210), TRUE);
    MoveWindow(preview.modeCombo, preview.Px(452), preview.Px(10), preview.Px(116), preview.Px(140), TRUE);
    MoveWindow(preview.fullscreenButton, preview.Px(578), preview.Px(9), preview.Px(84), preview.Px(26), TRUE);
    UpdateScrollbars(window, preview);
}

void ToggleFullscreen(HWND window, Preview& preview)
{
    if (preview.fullscreen) {
        preview.fullscreen = false;
        SetWindowLongPtrW(window, GWL_STYLE, GetWindowLongPtrW(window, GWL_STYLE) | WS_OVERLAPPEDWINDOW);
        SetWindowPlacement(window, &preview.windowed);
        SetWindowPos(window, nullptr, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    } else {
        MONITORINFO monitor{sizeof(monitor)};
        if (!GetWindowPlacement(window, &preview.windowed)
            || !GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &monitor)) return;
        preview.fullscreen = true;
        SetWindowLongPtrW(window, GWL_STYLE, GetWindowLongPtrW(window, GWL_STYLE) & ~static_cast<LONG_PTR>(WS_OVERLAPPEDWINDOW));
        const auto& bounds = monitor.rcMonitor;
        SetWindowPos(window, nullptr, bounds.left, bounds.top, bounds.right - bounds.left, bounds.bottom - bounds.top,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
    SetWindowTextW(preview.fullscreenButton, preview.fullscreen ? L"退出全屏" : L"全屏");
    UpdateScrollbars(window, preview);
}

LRESULT CALLBACK WindowProc(HWND window, UINT message, WPARAM key, LPARAM data)
{
    auto* preview = reinterpret_cast<Preview*>(GetWindowLongPtrW(window, GWLP_USERDATA));
    switch (message) {
    case WM_NCCREATE: {
        const auto* create = reinterpret_cast<const CREATESTRUCTW*>(data);
        SetWindowLongPtrW(window, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(create->lpCreateParams));
        return DefWindowProcW(window, message, key, data);
    }
    case WM_PAINT:
        Paint(window, *preview);
        return 0;
    case WM_COMMAND:
        if (LOWORD(key) == sampleId && HIWORD(key) == CBN_SELCHANGE) {
            const auto index = static_cast<size_t>(SendMessageW(preview->combo, CB_GETCURSEL, 0, 0));
            if (index < samples.size()) {
                preview->Select(index);
                InvalidateRect(window, nullptr, FALSE);
            }
        }
        if (LOWORD(key) == sizeId && HIWORD(key) == CBN_SELCHANGE && preview->mode == PreviewMode::formats) {
            const auto index = SendMessageW(preview->sizeCombo, CB_GETCURSEL, 0, 0);
            if (index >= 0 && index <= static_cast<LRESULT>(largeSizes.size())) {
                preview->largeSize = index ? largeSizes[static_cast<size_t>(index - 1)] : 0;
                preview->Select(preview->sample);
                UpdateScrollbars(window, *preview, true);
            }
        }
        if (LOWORD(key) == modeId && HIWORD(key) == CBN_SELCHANGE) {
            const auto index = SendMessageW(preview->modeCombo, CB_GETCURSEL, 0, 0);
            if (index < 0 || index > static_cast<LRESULT>(PreviewMode::formats)) return 0;
            preview->mode = static_cast<PreviewMode>(index);
            if (preview->mode != PreviewMode::formats) {
                preview->largeSize = 0;
                SendMessageW(preview->sizeCombo, CB_SETCURSEL, 0, 0);
            }
            EnableWindow(preview->sizeCombo, preview->mode == PreviewMode::formats);
            preview->Select(preview->sample);
            UpdateScrollbars(window, *preview, true);
        }
        if (LOWORD(key) == fullscreenId && HIWORD(key) == BN_CLICKED) ToggleFullscreen(window, *preview);
        return 0;
    case WM_SIZE:
        if (preview && preview->combo && key != SIZE_MINIMIZED) UpdateScrollbars(window, *preview);
        return 0;
    case WM_HSCROLL:
    case WM_VSCROLL:
        Scroll(window, message == WM_HSCROLL ? SB_HORZ : SB_VERT, LOWORD(key), preview->Px(36));
        return 0;
    case WM_MOUSEWHEEL:
    case WM_MOUSEHWHEEL: {
        const int delta = GET_WHEEL_DELTA_WPARAM(key);
        const bool horizontal = message == WM_MOUSEHWHEEL || (GET_KEYSTATE_WPARAM(key) & MK_SHIFT);
        const bool forward = message == WM_MOUSEHWHEEL ? delta > 0 : delta < 0;
        Scroll(window, horizontal ? SB_HORZ : SB_VERT, forward ? SB_LINEDOWN : SB_LINEUP,
            std::abs(delta) * preview->Px(90) / WHEEL_DELTA);
        return 0;
    }
    case WM_ERASEBKGND:
        return 1;
    case WM_DPICHANGED: {
        const auto& bounds = *reinterpret_cast<const RECT*>(data);
        if (!preview->fullscreen) {
            SetWindowPos(window, nullptr, bounds.left, bounds.top, bounds.right - bounds.left, bounds.bottom - bounds.top,
                SWP_NOZORDER | SWP_NOACTIVATE);
        }
        Layout(window, *preview);
        return 0;
    }
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    default:
        return DefWindowProcW(window, message, key, data);
    }
}

int ShowPreview(HINSTANCE instance)
{
    ComPtr<ID3D11Device> graphics;
    auto result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        nullptr, 0, D3D11_SDK_VERSION, graphics.GetAddressOf(), nullptr, nullptr);
    if (FAILED(result)) {
        result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr, 0, D3D11_SDK_VERSION, graphics.ReleaseAndGetAddressOf(), nullptr, nullptr);
    }
    Preview preview;
    ComPtr<IDXGIDevice> dxgi;
    ComPtr<ID2D1Device> device;
    ComPtr<ID2D1DeviceContext> canvas;
    if (FAILED(result) || FAILED(graphics.As(&dxgi))
        || FAILED(D2D1CreateDevice(dxgi.Get(), nullptr, device.GetAddressOf()))
        || FAILED(device->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE, canvas.GetAddressOf()))
        || FAILED(canvas.As(&preview.canvas))
        || FAILED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(preview.wic.GetAddressOf())))
        || FAILED(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
            reinterpret_cast<IUnknown**>(preview.write.GetAddressOf())))) return 1;

    WNDCLASSW windowInfo{};
    windowInfo.lpfnWndProc = WindowProc;
    windowInfo.hInstance = instance;
    windowInfo.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    windowInfo.lpszClassName = windowClass;
    if (!RegisterClassW(&windowInfo)) return 1;

    const HWND window = CreateWindowExW(0, windowClass, windowTitle, windowStyle,
        CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, nullptr, nullptr, instance, &preview);
    if (!window) return 1;
    preview.combo = CreateWindowExW(0, L"COMBOBOX", nullptr, WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST,
        0, 0, 1, 150, window, reinterpret_cast<HMENU>(static_cast<INT_PTR>(sampleId)), instance, nullptr);
    preview.sizeCombo = CreateWindowExW(0, L"COMBOBOX", nullptr, WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST,
        0, 0, 1, 150, window, reinterpret_cast<HMENU>(static_cast<INT_PTR>(sizeId)), instance, nullptr);
    preview.modeCombo = CreateWindowExW(0, L"COMBOBOX", nullptr, WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST,
        0, 0, 1, 150, window, reinterpret_cast<HMENU>(static_cast<INT_PTR>(modeId)), instance, nullptr);
    preview.fullscreenButton = CreateWindowExW(0, L"BUTTON", L"全屏", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
        0, 0, 1, 1, window, reinterpret_cast<HMENU>(static_cast<INT_PTR>(fullscreenId)), instance, nullptr);
    if (!preview.combo || !preview.sizeCombo || !preview.modeCombo || !preview.fullscreenButton) {
        DestroyWindow(window);
        return 1;
    }
    for (const auto& sample : samples) {
        SendMessageW(preview.combo, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(sample.label));
    }
    SendMessageW(preview.combo, CB_SETCURSEL, initialSample, 0);
    SendMessageW(preview.sizeCombo, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(L"16–72 px"));
    for (const int size : largeSizes) {
        const auto label = std::to_wstring(size) + L" px";
        SendMessageW(preview.sizeCombo, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(label.c_str()));
    }
    SendMessageW(preview.sizeCombo, CB_SETCURSEL, 0, 0);
    EnableWindow(preview.sizeCombo, FALSE);
    SendMessageW(preview.modeCombo, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(L"重构对比"));
    SendMessageW(preview.modeCombo, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(L"算法对比"));
    SendMessageW(preview.modeCombo, CB_ADDSTRING, 0, reinterpret_cast<LPARAM>(L"SVG / PNG"));
    SendMessageW(preview.modeCombo, CB_SETCURSEL, 0, 0);
    preview.Select(initialSample);
    Layout(window, preview);
    int height = preview.Px(headerHeight + padding);
    for (const int size : sizes) height += preview.RowHeight(size);
    RECT bounds{0, 0, preview.Px(clientWidth) + 1, height + 1};
    AdjustWindowRectExForDpi(&bounds, windowStyle, FALSE, 0, preview.dpi);
    MONITORINFO monitor{sizeof(monitor)};
    if (GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &monitor)) {
        const auto& work = monitor.rcWork;
        const auto width = std::min(bounds.right - bounds.left, work.right - work.left);
        const auto windowHeight = std::min(bounds.bottom - bounds.top, work.bottom - work.top);
        SetWindowPos(window, nullptr, work.left + (work.right - work.left - width) / 2,
            work.top + (work.bottom - work.top - windowHeight) / 2, width, windowHeight, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    ShowWindow(window, SW_SHOW);

    MSG message{};
    BOOL pending;
    while ((pending = GetMessageW(&message, nullptr, 0, 0)) > 0) {
        if (message.message == WM_KEYDOWN && message.wParam == VK_F11) {
            if (!(message.lParam & (1L << 30))) ToggleFullscreen(window, preview);
            continue;
        }
        if (message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE && preview.fullscreen
            && !SendMessageW(preview.combo, CB_GETDROPPEDSTATE, 0, 0)
            && !SendMessageW(preview.modeCombo, CB_GETDROPPEDSTATE, 0, 0)
            && !SendMessageW(preview.sizeCombo, CB_GETDROPPEDSTATE, 0, 0)) {
            ToggleFullscreen(window, preview);
            continue;
        }
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    return pending < 0 ? 1 : static_cast<int>(message.wParam);
}

}

int RunIconPreview(HINSTANCE instance)
{
    const auto com = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    if (FAILED(com)) return 1;
    const auto result = ShowPreview(instance);
    CoUninitialize();
    if (result) MessageBoxW(nullptr, L"无法打开图标对比窗口。", windowTitle, MB_OK | MB_ICONWARNING);
    return result;
}

}
