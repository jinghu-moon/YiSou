#include <blend2d/blend2d.h>
#include <iostream>
#include <string>
int main(int argc, char** argv) {
    if (argc != 2) return 1;
    const int size = std::stoi(argv[1]);
    BLImage image(size, size, BL_FORMAT_PRGB32);
    BLContext context(image);
    context.clear_all();
    unsigned color;
    while (std::cin >> std::hex >> color >> std::dec) {
        BLPath path;
        char command;
        while (std::cin >> command && command != 'E') {
            double x, y, u, v, a, b;
            if (command == 'Z') { path.close(); continue; }
            if (!(std::cin >> x >> y)) return 2;
            switch (command) {
            case 'M': path.move_to(x, y); break;
            case 'L': path.line_to(x, y); break;
            case 'Q': std::cin >> u >> v; path.quad_to(x, y, u, v); break;
            case 'C': std::cin >> u >> v >> a >> b; path.cubic_to(x, y, u, v, a, b); break;
            default: return 3;
            }
        }
        if (context.fill_path(path, BLRgba32(color)) != BL_SUCCESS) return 4;
    }
    if (context.end() != BL_SUCCESS) return 5;
    return image.write_to_file("blend-output.png") != BL_SUCCESS;
}
