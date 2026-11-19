"""Audit the icon preview against per-pixel area coverage.

Use the dependencies from generate_icon_algorithms.py plus resvg-py.
Build the Direct2D helper in an x64 Visual Studio developer shell, from
build/svg-small-test:
    cl /O2 /EHsc /std:c++20 /utf-8 /I../../src/core /DUNICODE /D_UNICODE /DNOMINMAX ../../tools/icon_algorithms/direct2d.cpp ../../src/platform/localization.cpp /link /out:direct2d.exe d3d11.lib d2d1.lib shlwapi.lib msimg32.lib windowscodecs.lib ole32.lib user32.lib gdi32.lib
Run from the workspace with python -B tools/benchmark_icon_algorithms.py.
Pass --refresh-preview to regenerate the comparison window's PNG sheets.
"""

from argparse import ArgumentParser
from csv import DictWriter
from io import BytesIO
from math import pi
from subprocess import run
from xml.etree import ElementTree as ET

import cairosvg
import numpy as np
from PIL import Image
import resvg_py

from generate_icon_algorithms import (
    freetype_image, generate, methods, prepare_sample, render_methods, shader_page,
)
from generate_icon_preview import root, sources, tabler, thesvg

work = root / "build/svg-small-test"
results = work / "svg-audit"
sizes = (12, 16, 18, 20, 24, 26, 34, 48, 72)
original_labels = ("Search", "Yandex", "YouTube Mono", "YouTube Color", "Google", "Bilibili", "Flame")
namespace = "http://www.w3.org/2000/svg"
ET.register_namespace("", namespace)


def path(data, attributes=""):
    return f'<path d="{data}" {attributes}/>'


def artwork(content, viewbox="0 0 24 24"):
    return f'<svg xmlns="{namespace}" viewBox="{viewbox}">{content}</svg>'


def fixtures():
    stroke = 'fill="none" stroke="black" stroke-linecap="round" stroke-linejoin="round"'
    return {
        "Pixel-aligned rectangle": path("M6 6H18V18H6Z"),
        "Fractional rectangle": path("M5.25 6.5H19.75V18.75H5.25Z"),
        "Thin horizontal and vertical": path("M3 5H21 M4 9V21", f'{stroke} stroke-width=".55"'),
        "Diagonal and shallow stroke": path("M2 3L21 18 M3 20L21 21.7", f'{stroke} stroke-width=".85"'),
        "Quadratic curves": path("M2 12Q6 -1 12 12T22 12Q18 24 12 15T2 12Z"),
        "Cubic inflection": path("M2 20C0 0 24 26 22 4L19 5C18 17 5 -1 2 20Z"),
        "Elliptical arcs": path("M4 12A8 5 32 1 1 20 12A8 5 32 1 1 4 12Z"),
        "Sharp points": path("M12 1 14 8 22 5 17 12 23 16 15 16 12 23 9 16 1 16 7 12 2 5 10 8Z"),
        "Narrow gap": path("M4 4H11.8V20H4Z M12.2 4H20V20H12.2Z"),
        "Even-odd holes": path("M2 2H22V22H2Z M5 5H19V19H5Z M9 9H15V15H9Z", 'fill-rule="evenodd"'),
        "Nonzero winding overlap": path("M4 4H14V17H4Z M10 7H20V20H10Z"),
        "Self-crossing contour": path("M4 3L20 21H4L20 3Z", 'fill-rule="evenodd"'),
        "Shared diagonal colors": path("M2 2H22L2 22Z", 'fill="red"') + path("M22 2V22H2Z", 'fill="blue"'),
        "Opaque color overlap": path("M3 3H16V21H3Z", 'fill="#EF3232"') + path("M8 2L22 12 8 22Z", 'fill="#3240EF"'),
        "Translucent color overlap": path("M3 3H16V21H3Z", 'fill="red" fill-opacity=".5"') + path("M8 2L22 12 8 22Z", 'fill="blue" fill-opacity=".5"'),
        "Fill plus stroke": path("M4 5Q12 1 20 5L17 19H7Z", 'fill="#FFDF18" stroke="#004EAF" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"'),
        "Viewport clipping": path("M-3 6L21 2L27 15L6 22Z"),
    }


def samples():
    for label, source in zip(original_labels, sources, strict=True):
        yield "original", label, source
    for name in ("activity", "heart", "star", "cloud", "world", "moon", "settings", "atom"):
        yield "real", f"Tabler {name}", tabler / f"{name}.svg"
    for name, variant in (("github", "mono"), ("chrome", "default"), ("spotify", "default"),
                          ("discord", "default"), ("microsoft", "default")):
        yield "real", name, thesvg / name / f"{variant}.svg"
    for index, (label, content) in enumerate(fixtures().items()):
        source = results / f"fixture-{index:02}.svg"
        source.write_text(artwork(content), encoding="utf-8")
        yield "fixture", label, source


def render_svg(text, size, renderer):
    document = ET.fromstring(text)
    document.attrib.update(width=str(size), height=str(size))
    markup = ET.tostring(document, encoding="unicode")
    if renderer == "resvg":
        png = resvg_py.svg_to_bytes(svg_string=markup, skip_system_fonts=True)
    else:
        png = cairosvg.svg2png(bytestring=markup.encode())
    with Image.open(BytesIO(png)) as image:
        if image.size != (size, size):
            raise ValueError(f"Unexpected {renderer} viewport {image.size}")
        return image.convert("RGBA")


def premultiplied(image):
    return np.asarray(image.convert("RGBa"), dtype=np.float64)


def area_reference(svg_text, size, factor, renderer):
    image = render_svg(svg_text, size * factor, renderer)
    # Average premultiplied sRGB and alpha in exact output-pixel bins.
    pixels = np.asarray(image.convert("RGBa"))
    return pixels.reshape(size, factor, size, factor, 4).mean(axis=(1, 3))


def metrics(image, reference):
    black = image[:, :, :3] - reference[:, :, :3]
    alpha = image[:, :, 3] - reference[:, :, 3]
    white = np.abs(black - alpha[:, :, None])
    mask = (reference[:, :, 3] > 0.5) & (reference[:, :, 3] < 254.5)
    for axis in (0, 1):
        changes = np.max(np.abs(np.diff(reference, axis=axis)), axis=2) >= 1
        if axis == 0:
            mask[:-1] |= changes
            mask[1:] |= changes
        else:
            mask[:, :-1] |= changes
            mask[:, 1:] |= changes
    return {
        "white_mae": white.mean(),
        "black_mae": np.abs(black).mean(),
        "alpha_mae": np.abs(alpha).mean(),
        "edge_white_mae": white[mask].mean() if mask.any() else None,
        "white_max": white.max(),
        "white_delta_16_pct": 100 * np.mean(np.max(white, axis=2) >= 16),
    }


def exact_cases():
    unit = "0 0 1 1"
    black = lambda coverage: [0, 0, 0, 255 * coverage]
    yield "Quarter pixel", artwork(path("M.1 .25H.6V.75H.1Z"), unit), black(1 / 4)
    yield "Triangle half pixel", artwork(path("M0 0H1L0 1Z"), unit), black(1 / 2)
    yield "Quadratic integral", artwork(path("M0 0Q.5 0 1 1H0Z"), unit), black(2 / 3)
    yield "Cubic integral", artwork(path("M0 0C.3333333333 0 .6666666667 0 1 1H0Z"), unit), black(3 / 4)
    yield "Circle area", artwork(path("M.25 .5A.25 .25 0 1 0 .75 .5A.25 .25 0 1 0 .25 .5Z"), unit), black(pi / 16)
    yield "Even-odd area", artwork(path("M0 0H1V1H0Z M.25 .25H.75V.75H.25Z", 'fill-rule="evenodd"'), unit), black(3 / 4)
    yield "Adjacent colors", artwork(path("M0 0H1L0 1Z", 'fill="red"') + path("M1 0V1H0Z", 'fill="blue"'), unit), [127.5, 0, 127.5, 255]
    yield "Translucent source-over", artwork(path("M0 0H1V1H0Z", 'fill="red" fill-opacity=".5"') + path("M0 0H1V1H0Z", 'fill="blue" fill-opacity=".5"'), unit), [63.75, 0, 127.5, 191.25]


def exact_checks(page):
    rows = []
    for i, (name, svg, expected) in enumerate(exact_cases()):
        source = results / f"exact-{i}.svg"
        source.write_text(svg, encoding="utf-8")
        if not np.allclose(metrics(np.zeros((1, 1, 4)), np.array(expected).reshape(1, 1, 4))["white_mae"],
                           np.abs(np.array(expected[:3]) - expected[3]).mean()):
            raise AssertionError("RGB compositing self-check")
        for renderer in ("resvg", "cairo"):
            actual = area_reference(svg, 1, 256, renderer)[0, 0]
            error = np.max(np.abs(actual - expected))
            if error > 1.25:
                raise AssertionError((renderer, name, actual, expected))
            rows.append({"case": name, "method": f"{renderer} 256x", "max_error": error})
        d2d, = read_direct2d(source, "exact", (1,))
        rows.append({"case": name, "method": "Direct2D SVG",
                     "max_error": np.max(np.abs(d2d[0, 0] - expected))})
        native = premultiplied(render_svg(svg, 1, "cairo"))
        rows.append({"case": name, "method": "Native PNG (CairoSVG)",
                     "max_error": np.max(np.abs(native[0, 0] - expected))})
        layers, glyphs = prepare_sample(source)
        for method, image in zip(methods, render_methods(page, 1, layers, glyphs), strict=True):
            error = np.max(np.abs(premultiplied(image)[0, 0] - expected))
            if method == "Manson-Schaefer" and name != "Adjacent colors" and error > 1.25:
                raise AssertionError((method, name, error))
            rows.append({"case": name, "method": method, "max_error": error})
    return rows


def read_direct2d(source, slot, target_sizes=sizes):
    folder = results / f"d2d-{slot}"
    folder.mkdir(exist_ok=True)
    run([work / "direct2d.exe", source, folder, *map(str, target_sizes)], check=True)
    for size in target_sizes:
        raw = np.fromfile(folder / f"{size}.raw", dtype=np.uint8).reshape(size, size, 4)
        if np.any(raw[:, :, :3] > raw[:, :, 3:]):
            raise AssertionError("Direct2D output is not premultiplied")
        yield raw[:, :, [2, 1, 0, 3]].astype(np.float64)


def canonical_svg(layers):
    return artwork("".join(
        path(outline.serialize(1), f'fill="rgb({r},{g},{b})" fill-opacity="{a / 255:.9g}"')
        for (r, g, b, a), outline in layers
    ), "0 0 1 1")


def compare(page):
    measurements = []
    validation = []
    for slot, (group, label, source) in enumerate(samples()):
        svg = source.read_text(encoding="utf-8")
        layers, glyphs = prepare_sample(source)
        canonical = canonical_svg(layers)
        for size, d2d in zip(sizes, read_direct2d(source, slot), strict=True):
            factor = 128 if size <= 24 else 64
            reference = area_reference(svg, size, factor, "resvg")
            info = {"group": group, "case": label, "size_px": size}
            if size in (16, 24, 34):
                checks = (
                    ("Cairo cross-check", area_reference(svg, size, factor, "cairo")),
                    ("resvg half factor", area_reference(svg, size, factor // 2, "resvg")),
                    ("Shared outline import", area_reference(canonical, size, factor, "resvg")),
                )
                for method, image in checks:
                    validation.append({**info, "method": method, **metrics(image, reference)})
            images = (
                ("Direct2D SVG", d2d),
                ("Native PNG (CairoSVG)", premultiplied(render_svg(svg, size, "cairo"))),
                ("FreeType Unhinted", premultiplied(freetype_image(size, glyphs, hinting=False))),
                *((method, premultiplied(image))
                  for method, image in zip(methods, render_methods(page, size, layers, glyphs), strict=True)),
            )
            for method, image in images:
                measurements.append({**info, "method": method, **metrics(image, reference)})
        print(f"{slot + 1:2} {group:8} {label}", flush=True)
    return measurements, validation


def report(filename, rows):
    with (results / filename).open("w", encoding="utf-8-sig", newline="") as handle:
        writer = DictWriter(handle, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


def summary(rows, title):
    print(title)
    groups = {}
    for row in rows:
        groups.setdefault(row["method"], []).append(row)
    averages = [(name, *(np.mean([item[key] for item in group if item[key] is not None])
                         for key in ("white_mae", "edge_white_mae", "black_mae", "alpha_mae")))
                for name, group in groups.items()]
    for method, white, edge, black, alpha in sorted(averages, key=lambda item: item[1]):
        print(f"{method:24} {white:7.3f} {edge:7.3f} {black:7.3f} {alpha:7.3f}")


def main():
    parser = ArgumentParser(description=__doc__)
    parser.add_argument("--refresh-preview", action="store_true")
    options = parser.parse_args()
    results.mkdir(parents=True, exist_ok=True)
    with shader_page() as page:
        report("exact-area-checks.csv", exact_checks(page))
        print("Analytic reference checks passed", flush=True)
        if options.refresh_preview:
            generate(page)
        rows, checks = compare(page)
    report("pixel-errors.csv", rows)
    report("reference-checks.csv", checks)
    summary(rows, "All samples")
    summary([row for row in rows if row["group"] == "original" and 16 <= row["size_px"] <= 24], "Original seven at 16-24 px")
    summary([row for row in rows if row["group"] != "fixture"], "Real icons")
    summary([row for row in rows if row["group"] == "fixture"], "Stress fixtures")
    summary(checks, "Reference and geometry cross-checks")


if __name__ == "__main__":
    main()
