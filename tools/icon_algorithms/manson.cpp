#include "rasterizer.h"
#include "filter_box.h"
#include <algorithm>
#include <fstream>
#include <iostream>
#include <string>

template<int Count>
void Add(Array<Curve<Count, 2>>& curves, const double (&coordinates)[Count * 2]) {
    Curve<Count, 2> curve;
    for (int i = 0; i < Count; ++i) curve.p[i].set(coordinates[i * 2], coordinates[i * 2 + 1]);
    curves.push_back(curve);
}

int main(int argc, char** argv) {
    if (argc != 2) return 1;
    const int size = std::stoi(argv[1]);
    Array<Curve<2, 2>> lines;
    Array<Curve<3, 2>> quads;
    Array<Curve<4, 2>> cubics;
    Array<Curve<3, 3>> rational;
    char command;
    double x = 0, y = 0, startX = 0, startY = 0;
    while (std::cin >> command) {
        if (command == 'Z') {
            if (startX != x || startY != y) Add<2>(lines, {x, y, startX, startY});
            x = startX;
            y = startY;
            continue;
        }
        double u, v, a, b, c, d;
        if (!(std::cin >> u >> v)) return 2;
        switch (command) {
        case 'M': x = startX = u; y = startY = v; break;
        case 'L': Add<2>(lines, {x, y, u, v}); x = u; y = v; break;
        case 'Q': std::cin >> a >> b; Add<3>(quads, {x, y, u, v, a, b}); x = a; y = b; break;
        case 'C': std::cin >> a >> b >> c >> d; Add<4>(cubics, {x, y, u, v, a, b, c, d}); x = c; y = d; break;
        default: return 3;
        }
    }
    Array2D<vect<1, double>> image;
    image.resize(size, size);
    for (int row = 0; row < size; ++row) for (int col = 0; col < size; ++col) image(col, row)[0] = 0;
    ColorFunction<1, 1> white;
    white.comp[0].c[0] = 1;
    FilterBox<1> box;
    RasterizerInstance<1, 1, 1> raster(image, lines.s != 0, quads.s != 0, cubics.s != 0, false);
    rasterize(raster, white, &lines, box.filter22, &quads, box.filter32, &cubics, box.filter42, &rational, box.filter33);
    std::ofstream output("manson-output.raw", std::ios::binary);
    for (int row = 0; row < size; ++row) for (int col = 0; col < size; ++col) {
        output.put(static_cast<char>(std::clamp(std::abs(image(col, row)[0]), 0.0, 1.0) * 255 + 0.5));
    }
    return !output;
}
