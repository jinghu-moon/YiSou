#include "application.h"

#include <d2d1_3.h>
#include <shlwapi.h>
#include <wrl/client.h>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iterator>

#include <d3d11.h>

namespace {
using yisou::IconPixels;
using Microsoft::WRL::ComPtr;

IconPixels RenderSvg(ID2D1DeviceContext5& iconContext, std::string_view svg, unsigned size, COLORREF color)
{
    if (svg.empty() || !size) return {};

    ComPtr<IStream> stream;
    stream.Attach(SHCreateMemStream(reinterpret_cast<const BYTE*>(svg.data()), static_cast<UINT>(svg.size())));
    const float edge = static_cast<float>(size);
    ComPtr<ID2D1SvgDocument> document;
    if (!stream || FAILED(iconContext.CreateSvgDocument(stream.Get(), D2D1::SizeF(edge, edge),
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
    if (FAILED(iconContext.CreateBitmap(D2D1::SizeU(size, size), nullptr, 0, &targetProperties, target.GetAddressOf()))
        || FAILED(iconContext.CreateBitmap(D2D1::SizeU(size, size), nullptr, 0, &readbackProperties, readback.GetAddressOf()))) {
        return {};
    }

    iconContext.SetTarget(target.Get());
    iconContext.BeginDraw();
    iconContext.Clear(D2D1::ColorF(0.f, 0.f, 0.f, 0.f));
    iconContext.DrawSvgDocument(document.Get());
    const auto drawn = iconContext.EndDraw();
    iconContext.SetTarget(nullptr);
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

}

int wmain(int argc, wchar_t** argv)
{
    if (argc < 4) return 1;

    Microsoft::WRL::ComPtr<ID3D11Device> graphics;
    Microsoft::WRL::ComPtr<IDXGIDevice> dxgi;
    Microsoft::WRL::ComPtr<ID2D1Device> device;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, nullptr, 0, D3D11_SDK_VERSION,
            graphics.GetAddressOf(), nullptr, nullptr))
        || FAILED(graphics.As(&dxgi))
        || FAILED(D2D1CreateDevice(dxgi.Get(), nullptr, device.GetAddressOf()))) return 2;

    std::ifstream input(argv[1], std::ios::binary);
    if (!input) return 3;
    const std::string svg(std::istreambuf_iterator<char>{input}, {});
    ComPtr<ID2D1DeviceContext> context;
    ComPtr<ID2D1DeviceContext5> svgContext;
    if (FAILED(device->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE, context.GetAddressOf()))
        || FAILED(context.As(&svgContext))) return 2;
    for (int index = 3; index < argc; ++index) {
        const auto size = static_cast<unsigned>(std::stoul(argv[index]));
        const auto pixels = RenderSvg(*svgContext.Get(), svg, size, CLR_INVALID);
        if (pixels.size() != static_cast<size_t>(size) * size) return 4;
        const auto file = std::filesystem::path(argv[2]) / (std::to_wstring(size) + L".raw");
        std::ofstream output(file, std::ios::binary);
        if (!output.write(reinterpret_cast<const char*>(pixels.data()),
                static_cast<std::streamsize>(pixels.size() * sizeof(std::uint32_t)))) return 5;
    }
}
