#include "engine_icons.h"

#include <filesystem>
#include <fstream>
#include <iterator>

int wmain(int argc, wchar_t** argv)
{
    if (argc < 4) return 1;
    std::ifstream input(argv[1], std::ios::binary);
    if (!input) return 2;
    const std::string svg(std::istreambuf_iterator<char>{input}, {});
    for (int index = 3; index < argc; ++index) {
        const auto size = static_cast<unsigned>(std::stoul(argv[index]));
        const auto pixels = yisou::ui::RasterizeSvgIcon(svg, size);
        if (pixels.size() != static_cast<size_t>(size) * size) return 3;
        const auto file = std::filesystem::path(argv[2]) / (std::to_wstring(size) + L".raw");
        std::ofstream output(file, std::ios::binary);
        if (!output.write(reinterpret_cast<const char*>(pixels.data()),
                static_cast<std::streamsize>(pixels.size() * sizeof(std::uint32_t)))) return 4;
    }
}
