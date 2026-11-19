#include "../platform/localization.h"
#include "engine_icons.h"
#include "../platform/performance.h"

#include <wincodec.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <list>
#include <map>
#include <tuple>

namespace yisou::ui {
using i18n::Text;
using i18n::Tr;

namespace {
using Microsoft::WRL::ComPtr;

ComPtr<IWICImagingFactory> ImageFactory()
{
    ComPtr<IWICImagingFactory> factory;
    CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(factory.GetAddressOf()));
    return factory;
}

IconPixels DecodeIcon(IWICImagingFactory& factory, IWICBitmapDecoder& decoder, unsigned size)
{
    ComPtr<IWICBitmapFrameDecode> frame;
    UINT width = 0, height = 0;
    if (FAILED(decoder.GetFrame(0, frame.GetAddressOf())) || FAILED(frame->GetSize(&width, &height))
        || !width || !height || width > 4096 || height > 4096) return {};

    const UINT largest = std::max(width, height);
    width = std::max(1u, (width * size + largest / 2) / largest);
    height = std::max(1u, (height * size + largest / 2) / largest);
    ComPtr<IWICBitmapScaler> scaler;
    ComPtr<IWICFormatConverter> converter;
    if (FAILED(factory.CreateBitmapScaler(scaler.GetAddressOf()))
        || FAILED(scaler->Initialize(frame.Get(), width, height, WICBitmapInterpolationModeFant))
        || FAILED(factory.CreateFormatConverter(converter.GetAddressOf()))
        || FAILED(converter->Initialize(scaler.Get(), GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone, nullptr, 0., WICBitmapPaletteTypeCustom))) return {};

    IconPixels scaled(width * height);
    const UINT stride = width * sizeof(std::uint32_t);
    if (FAILED(converter->CopyPixels(nullptr, stride, height * stride, reinterpret_cast<BYTE*>(scaled.data())))) return {};
    IconPixels pixels(size * size);
    const UINT offset = (size - height) / 2 * size + (size - width) / 2;
    for (UINT row = 0; row < height; ++row) {
        std::copy_n(scaled.data() + row * width, width, pixels.data() + offset + row * size);
    }
    return pixels;
}


std::string_view ResourceBytes(const wchar_t* name)
{
    const HMODULE module = GetModuleHandleW(nullptr);
    const HRSRC resource = FindResourceW(module, name, RT_RCDATA);
    if (!resource) return {};
    const auto* data = static_cast<const char*>(LockResource(LoadResource(module, resource)));
    return data ? std::string_view(data, SizeofResource(module, resource)) : std::string_view{};
}

std::string SvgColor(COLORREF color)
{
    constexpr char digits[] = "0123456789abcdef";
    std::string hex = "#";
    for (const unsigned channel : {GetRValue(color), GetGValue(color), GetBValue(color)}) {
        hex += digits[channel >> 4];
        hex += digits[channel & 15];
    }
    return hex;
}

using IconKey = std::tuple<std::string_view, unsigned, COLORREF>;
constexpr COLORREF bitmapKey = 0xfe000000;
constexpr size_t cacheLimit = 2 * 1024 * 1024;

struct CachedIcon {
    std::string source;
    unsigned size;
    COLORREF color;
    SharedIcon pixels;
    size_t bytes;
};

// The UI thread owns the cache. Index keys refer to stable list entries.
std::list<CachedIcon> iconLru;
std::map<IconKey, std::list<CachedIcon>::iterator> iconIndex;
size_t cacheBytes = 0;

template<class Render>
SharedIcon CachedPixels(const IconKey& key, Render render)
{
    const auto [source, size, color] = key;
    if (source.empty() || !size || size > maxSvgIconSize) return {};
    if (const auto match = iconIndex.find(key); match != iconIndex.end()) {
        YISOU_PERF_COUNT(svgCacheHit);
        iconLru.splice(iconLru.begin(), iconLru, match->second);
        return match->second->pixels;
    }
    YISOU_PERF_COUNT(svgCacheMiss);
    auto pixels = render();
    if (pixels.empty()) return {};
    auto result = std::make_shared<const IconPixels>(std::move(pixels));
    const size_t bytes = source.size() + result->size() * sizeof(std::uint32_t);
    while (cacheBytes + bytes > cacheLimit && !iconLru.empty()) {
        const auto last = std::prev(iconLru.end());
        iconIndex.erase(IconKey{last->source, last->size, last->color});
        cacheBytes -= last->bytes;
        iconLru.erase(last);
    }
    auto entry = iconLru.emplace(iconLru.begin(), CachedIcon{std::string(source), size, color, result, bytes});
    iconIndex.emplace(IconKey{entry->source, size, color}, entry);
    cacheBytes += bytes;
    return result;
}

SharedIcon SvgPixels(std::string_view svg, unsigned size, COLORREF color = CLR_INVALID)
{
    return CachedPixels({svg, size, color}, [&] {
        auto pixels = RasterizeSvgIcon(svg, size, color);
        if (std::ranges::none_of(pixels, [](auto pixel) { return pixel >> 24; })) pixels.clear();
        return pixels;
    });
}

std::string SolidSvg(std::uint32_t color)
{
    const auto hex = SvgColor(RGB(color >> 16, color >> 8, color));
    return "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 32\">"
        "<rect width=\"32\" height=\"32\" rx=\"6\" fill=\"" + hex + "\"/>"
        "<g transform=\"translate(5 5) scale(.916667)\" fill=\"none\" stroke=\"white\" stroke-width=\"2\" stroke-linecap=\"round\">"
        "<path d=\"M3 10a7 7 0 1 0 14 0a7 7 0 1 0 -14 0 M21 21l-6 -6\"/>"
        "</g></svg>";
}

RECT SquareBounds(const RECT& bounds)
{
    const LONG size = std::clamp(std::min(bounds.right - bounds.left, bounds.bottom - bounds.top),
        0L, static_cast<LONG>(maxSvgIconSize));
    const LONG x = bounds.left + (bounds.right - bounds.left - size) / 2;
    const LONG y = bounds.top + (bounds.bottom - bounds.top - size) / 2;
    return {x, y, x + size, y + size};
}

}

void DrawIconPixels(HDC dc, std::span<const std::uint32_t> pixels, const RECT& bounds, unsigned size)
{
    if (!size || pixels.size() != static_cast<size_t>(size) * size || IsRectEmpty(&bounds)) return;
    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = static_cast<LONG>(size);
    info.bmiHeader.biHeight = -static_cast<LONG>(size);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    void* bits = nullptr;
    const HBITMAP bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &bits, nullptr, 0);
    if (!bitmap) return;
    const HDC buffer = CreateCompatibleDC(dc);
    if (buffer) {
        std::memcpy(bits, pixels.data(), pixels.size_bytes());
        const HGDIOBJ previous = SelectObject(buffer, bitmap);
        AlphaBlend(dc, bounds.left, bounds.top, bounds.right - bounds.left, bounds.bottom - bounds.top,
            buffer, 0, 0, static_cast<int>(size), static_cast<int>(size), {AC_SRC_OVER, 0, 255, AC_SRC_ALPHA});
        SelectObject(buffer, previous);
        DeleteDC(buffer);
    }
    DeleteObject(bitmap);
}

const wchar_t* GroupName(EngineGroup group)
{
    switch (group) {
    case EngineGroup::web: return Tr(Text::webGroup);
    case EngineGroup::video: return Tr(Text::videoGroup);
    case EngineGroup::social: return Tr(Text::socialGroup);
    case EngineGroup::development: return Tr(Text::developmentGroup);
    case EngineGroup::shopping: return Tr(Text::shoppingGroup);
    case EngineGroup::custom: return Tr(Text::customGroup);
    }
    return L"";
}

IconPixels ReadIconFile(const wchar_t* path)
{
    const auto factory = ImageFactory();
    ComPtr<IWICBitmapDecoder> decoder;
    if (!factory || FAILED(factory->CreateDecoderFromFilename(path, nullptr, GENERIC_READ,
        WICDecodeMetadataCacheOnDemand, decoder.GetAddressOf()))) return {};
    return DecodeIcon(*factory.Get(), *decoder.Get(), iconSize);
}

extern "C" std::uint32_t yisou_svg_render(const char* svg, size_t length, std::uint32_t size,
    std::uint32_t rgb, std::uint32_t* output, size_t count);

IconPixels RasterizeSvgIcon(std::string_view svg, unsigned size, COLORREF color)
{
    if (svg.empty() || svg.size() > svgIconLimit || !size || size > maxSvgIconSize) return {};
    YISOU_PERF_SCOPE("render_svg");
    const auto rgb = color == CLR_INVALID ? UINT32_MAX
        : (static_cast<std::uint32_t>(GetRValue(color)) << 16)
            | (static_cast<std::uint32_t>(GetGValue(color)) << 8) | GetBValue(color);
    IconPixels output(static_cast<size_t>(size) * size);
    if (!yisou_svg_render(svg.data(), svg.size(), size, rgb, output.data(), output.size())) return {};
    return output;
}

std::string ReadSvgIconFile(const wchar_t* path)
{
    std::ifstream input(std::filesystem::path(path), std::ios::binary | std::ios::ate);
    const auto length = input.tellg();
    if (length <= 0 || length > svgIconLimit) return {};
    std::string svg(static_cast<size_t>(length), '\0');
    if (!input.seekg(0) || !input.read(svg.data(), static_cast<std::streamsize>(svg.size()))) return {};
    if (!SvgPixels(svg, 64)) return {};
    return svg;
}

SharedIcon EngineIcon(const Engine& engine, unsigned size)
{
    if (engine.icon.size() == iconSize * iconSize) {
        const std::string_view data(reinterpret_cast<const char*>(engine.icon.data()),
            engine.icon.size() * sizeof(std::uint32_t));
        return CachedPixels({data, iconSize, bitmapKey}, [&] { return engine.icon; });
    }
    const auto svg = engine.group == EngineGroup::custom ? std::string_view(engine.svgIcon) : ResourceBytes(engine.id.c_str());
    if (const auto pixels = SvgPixels(svg, size)) return pixels;
    return SvgPixels(SolidSvg(engine.color), size);
}

void DrawEngineIcon(HDC dc, const Engine& engine, const RECT& bounds)
{
    const auto square = SquareBounds(bounds);
    const auto size = static_cast<unsigned>(square.right - square.left);
    if (const auto pixels = EngineIcon(engine, size)) {
        DrawIconPixels(dc, *pixels, square, engine.icon.empty() ? size : iconSize);
    }
}

SharedIcon TablerPixels(TablerIcon icon, COLORREF color, unsigned size)
{
    constexpr std::array names{L"browser", L"check", L"chevron_down", L"chevron_up", L"eye", L"file_typography",
        L"keyboard", L"palette", L"search", L"settings", L"typography", L"history", L"plus"};
    static_assert(names.size() == static_cast<size_t>(TablerIcon::count));
    const auto index = static_cast<size_t>(icon);
    return index < names.size() ? SvgPixels(ResourceBytes(names[index]), size, color) : SharedIcon{};
}

void DrawTablerIcon(HDC dc, TablerIcon icon, const RECT& bounds, COLORREF color)
{
    const auto square = SquareBounds(bounds);
    const auto size = static_cast<unsigned>(square.right - square.left);
    if (const auto pixels = TablerPixels(icon, color, size)) DrawIconPixels(dc, *pixels, square, size);
}

}
