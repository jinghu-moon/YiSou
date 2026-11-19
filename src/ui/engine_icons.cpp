#include "../platform/localization.h"
#include "engine_icons.h"
#include "../platform/performance.h"

#include <d2d1_3.h>
#include <shlwapi.h>
#include <wincodec.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <map>

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

ComPtr<ID2D1DeviceContext5> iconContext;
std::map<std::string, std::map<std::pair<unsigned, COLORREF>, IconPixels>, std::less<>> renderedIcons;
size_t cacheBytes = 0;
constexpr unsigned maxRenderSize = 256;
constexpr size_t cacheLimit = 2 * 1024 * 1024;

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

IconPixels RenderSvg(std::string_view svg, unsigned size, COLORREF color)
{
    if (!iconContext || svg.empty() || !size || size > maxRenderSize) return {};
    YISOU_PERF_SCOPE("render_svg");

    ComPtr<IStream> stream;
    stream.Attach(SHCreateMemStream(reinterpret_cast<const BYTE*>(svg.data()), static_cast<UINT>(svg.size())));
    const float edge = static_cast<float>(size);
    ComPtr<ID2D1SvgDocument> document;
    if (!stream || FAILED(iconContext->CreateSvgDocument(stream.Get(), D2D1::SizeF(edge, edge),
        document.GetAddressOf()))) return {};

    ComPtr<ID2D1SvgElement> root;
    document->GetRoot(root.GetAddressOf());
    if (!root) return {};
    if (color != CLR_INVALID && FAILED(root->SetAttributeValue(L"color",
        D2D1::ColorF(GetRValue(color) / 255.f, GetGValue(color) / 255.f, GetBValue(color) / 255.f)))) return {};
    const auto number = D2D1_SVG_LENGTH_UNITS_NUMBER;
    if (!root->IsAttributeSpecified(L"viewBox")) {
        D2D1_SVG_LENGTH width{}, height{};
        if (SUCCEEDED(root->GetAttributeValue(L"width", &width)) && SUCCEEDED(root->GetAttributeValue(L"height", &height))
            && width.units == number && height.units == number && width.value > 0 && height.value > 0) {
            const D2D1_SVG_VIEWBOX viewBox{0, 0, width.value, height.value};
            if (FAILED(root->SetAttributeValue(L"viewBox", D2D1_SVG_ATTRIBUTE_POD_TYPE_VIEWBOX,
                &viewBox, sizeof(viewBox)))) return {};
        }
    }
    const D2D1_SVG_LENGTH viewport{edge, number};
    if (FAILED(root->SetAttributeValue(L"width", viewport)) || FAILED(root->SetAttributeValue(L"height", viewport))) return {};

    const auto format = D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED);
    const auto targetProperties = D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_TARGET, format, 96, 96);
    const auto readbackProperties = D2D1::BitmapProperties1(
        D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW, format, 96, 96);
    ComPtr<ID2D1Bitmap1> target, readback;
    if (FAILED(iconContext->CreateBitmap(D2D1::SizeU(size, size), nullptr, 0, &targetProperties, target.GetAddressOf()))
        || FAILED(iconContext->CreateBitmap(D2D1::SizeU(size, size), nullptr, 0, &readbackProperties, readback.GetAddressOf()))) {
        return {};
    }

    iconContext->SetTarget(target.Get());
    iconContext->BeginDraw();
    iconContext->Clear(D2D1::ColorF(0.f, 0.f, 0.f, 0.f));
    iconContext->DrawSvgDocument(document.Get());
    const auto drawn = iconContext->EndDraw();
    iconContext->SetTarget(nullptr);
    D2D1_MAPPED_RECT mapped{};
    if (FAILED(drawn) || FAILED(readback->CopyFromBitmap(nullptr, target.Get(), nullptr))
        || FAILED(readback->Map(D2D1_MAP_OPTIONS_READ, &mapped))) return {};

    IconPixels pixels(static_cast<size_t>(size) * size);
    const auto stride = size * sizeof(std::uint32_t);
    for (unsigned row = 0; row < size; ++row) {
        std::memcpy(pixels.data() + static_cast<size_t>(row) * size, mapped.bits + row * mapped.pitch, stride);
    }
    readback->Unmap();
    return pixels;
}

std::span<const std::uint32_t> SvgPixels(std::string_view svg, unsigned size, COLORREF color = CLR_INVALID)
{
    const auto key = std::pair{size, color};
    if (const auto image = renderedIcons.find(svg); image != renderedIcons.end()) {
        if (const auto pixels = image->second.find(key); pixels != image->second.end()) {
            YISOU_PERF_COUNT(svgCacheHit);
            return pixels->second;
        }
    }
    YISOU_PERF_COUNT(svgCacheMiss);
    auto pixels = RenderSvg(svg, size, color);
    if (pixels.empty()) return {};
    const size_t bytes = svg.size() + pixels.size() * sizeof(std::uint32_t);
    if (cacheBytes + bytes > cacheLimit) {
        renderedIcons.clear();
        cacheBytes = 0;
    }
    cacheBytes += bytes;
    return renderedIcons[std::string(svg)].emplace(key, std::move(pixels)).first->second;
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
        0L, static_cast<LONG>(maxRenderSize));
    const LONG x = bounds.left + (bounds.right - bounds.left - size) / 2;
    const LONG y = bounds.top + (bounds.bottom - bounds.top - size) / 2;
    return {x, y, x + size, y + size};
}

}

void DrawIconPixels(HDC dc, std::span<const std::uint32_t> pixels, const RECT& bounds, unsigned size)
{
    if (pixels.empty() || IsRectEmpty(&bounds)) return;
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

void SetIconDevice(ID2D1Device* device)
{
    iconContext.Reset();
    if (!device) return;
    ComPtr<ID2D1DeviceContext> context;
    if (SUCCEEDED(device->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE, context.GetAddressOf()))) {
        context.As(&iconContext);
    }
}

std::string ReadSvgIconFile(const wchar_t* path)
{
    std::ifstream input(std::filesystem::path(path), std::ios::binary | std::ios::ate);
    const auto length = input.tellg();
    if (length <= 0 || length > svgIconLimit) return {};
    std::string svg(static_cast<size_t>(length), '\0');
    if (!input.seekg(0) || !input.read(svg.data(), static_cast<std::streamsize>(svg.size()))) return {};
    const auto pixels = RenderSvg(svg, 64, CLR_INVALID);
    if (std::ranges::none_of(pixels, [](auto pixel) { return pixel >> 24; })) return {};
    return svg;
}

std::span<const std::uint32_t> EngineIcon(const Engine& engine, unsigned size)
{
    if (!engine.icon.empty()) return engine.icon;
    const auto svg = engine.group == EngineGroup::custom ? std::string_view(engine.svgIcon) : ResourceBytes(engine.id.c_str());
    if (const auto pixels = SvgPixels(svg, size); !pixels.empty()) return pixels;
    return SvgPixels(SolidSvg(engine.color), size);
}

void DrawEngineIcon(HDC dc, const Engine& engine, const RECT& bounds)
{
    const auto square = SquareBounds(bounds);
    const auto size = static_cast<unsigned>(square.right - square.left);
    DrawIconPixels(dc, EngineIcon(engine, size), square, engine.icon.empty() ? size : iconSize);
}

std::span<const std::uint32_t> TablerPixels(TablerIcon icon, COLORREF color, unsigned size)
{
    constexpr std::array names{L"browser", L"check", L"chevron_down", L"chevron_up", L"eye", L"file_typography",
        L"keyboard", L"palette", L"search", L"settings", L"typography", L"history", L"plus"};
    static_assert(names.size() == static_cast<size_t>(TablerIcon::count));
    return SvgPixels(ResourceBytes(names[static_cast<size_t>(icon)]), size, color);
}

void DrawTablerIcon(HDC dc, TablerIcon icon, const RECT& bounds, COLORREF color)
{
    const auto square = SquareBounds(bounds);
    const auto size = static_cast<unsigned>(square.right - square.left);
    DrawIconPixels(dc, TablerPixels(icon, color, size), square, size);
}

}
