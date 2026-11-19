#include "../../src/ui/engine_icons.cpp"

#include <d3d11.h>

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
    yisou::ui::SetIconDevice(device.Get());
    for (int index = 3; index < argc; ++index) {
        const auto size = static_cast<unsigned>(std::stoul(argv[index]));
        // Use the same SVG rasterizer as EngineIcon, with errors visible to the benchmark.
        const auto pixels = yisou::ui::RenderSvg(svg, size, CLR_INVALID);
        if (pixels.size() != static_cast<size_t>(size) * size) return 4;
        const auto file = std::filesystem::path(argv[2]) / (std::to_wstring(size) + L".raw");
        std::ofstream output(file, std::ios::binary);
        if (!output.write(reinterpret_cast<const char*>(pixels.data()),
                static_cast<std::streamsize>(pixels.size() * sizeof(std::uint32_t)))) return 5;
    }
    yisou::ui::SetIconDevice(nullptr);
}
