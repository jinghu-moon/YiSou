r"""Generate comparison sheets using the local algorithm research builds.

Prepare target/icon-algorithms with:
  blend2d from https://github.com/blend2d/blend2d/tree/58ca9460
  windfoil from https://github.com/texel-org/windfoil/tree/f79ed320
  manson from https://josiahmanson.com/research/scanline_rasterization/supplemental/rasterizer2.zip

In an x64 Visual Studio developer shell, from target/icon-algorithms:
    cmake -S blend2d -B blend-build -DBLEND2D_NO_JIT=ON
    cmake --build blend-build --config Release
    copy blend-build\Release\blend2d.dll .
    cl /O2 /EHsc /std:c++17 /Iblend2d ..\..\tools\icon_algorithms\blend.cpp blend-build\Release\blend2d.lib /Feblend.exe
    cl /O2 /EHsc /std:c++17 /I manson\rasterizer2\rasterizer2 ..\..\tools\icon_algorithms\manson.cpp manson\rasterizer2\rasterizer2\filter_box.cpp manson\rasterizer2\rasterizer2\vect.cpp /Femanson.exe

Run with CairoSVG, Pillow, NumPy, fontTools, skia-pathops, freetype-py and
Playwright installed. Chrome runs the Windfoil shader and its Slug WGSL port.
Rasterizer2 is the non-commercial accompanying implementation of Josiah Manson
and Scott Schaefer's Analytic Rasterization of Curves with Polynomial Filters.
Slug shader algorithm and reference code copyright 2017 Eric Lengyel.
"""

from contextlib import contextmanager
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from io import BytesIO
from math import ceil
from os import environ
from pathlib import Path
from subprocess import run
from threading import Thread
from xml.etree import ElementTree

import cairocffi as cairo
import freetype
import numpy as np
import pathops
from fontTools.pens.basePen import BasePen
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.svgLib.path import parse_path
from fontTools.ttLib import TTFont
from PIL import Image, ImageColor
from playwright.sync_api import sync_playwright

from generate_icon_preview import output, root, sources

work = root / "target/icon-algorithms"
sizes = (16, 18, 20, 24, 30, 36, 42, 48, 64, 72)
methods = ("Blend2D", "Manson-Schaefer", "Joint 16x16", "FreeType AutoHint",
           "Windfoil", "Slug WGSL")
font_file = Path(environ.get("WINDIR", "C:/Windows")) / "Fonts/arial.ttf"
paint_attributes = {"color", "fill", "fill-opacity", "fill-rule", "stroke",
                    "stroke-opacity", "stroke-width", "stroke-linecap", "stroke-linejoin"}


class OutlineCommands(BasePen):
    def __init__(self, path):
        super().__init__(None)
        self.commands = []
        path.draw(self)

    def _moveTo(self, point):
        self.commands.append(("M", point))

    def _lineTo(self, point):
        self.commands.append(("L", point))

    def _qCurveToOne(self, control, end):
        self.commands.append(("Q", (*control, *end)))

    def _curveToOne(self, control1, control2, end):
        self.commands.append(("C", (*control1, *control2, *end)))

    def _closePath(self):
        self.commands.append(("Z", ()))

    def _endPath(self):
        self._closePath()

    def serialize(self, size, offset=0):
        return "\n".join(
            " ".join((command, *(f"{coord * size + offset:.10g}" for coord in points)))
            for command, points in self.commands
        )


def open_layers(source):
    svg = ElementTree.parse(source).getroot()
    x, y, width, height = (float(value) for value in svg.attrib["viewBox"].split())
    root_attributes = paint_attributes | {"viewBox", "width", "height", "preserveAspectRatio", "role",
                                          "{http://www.w3.org/XML/1998/namespace}space"}
    if set(svg.attrib) - root_attributes or svg.get("preserveAspectRatio") not in (None, "xMidYMid", "xMidYMid meet"):
        raise ValueError(f"Unsupported viewport or paint attributes in {source}")
    scale = 1 / max(width, height)
    transform = (scale, 0, 0, scale, (1 - width * scale) / 2 - x * scale,
                 (1 - height * scale) / 2 - y * scale)
    layers = []
    for element in svg:
        if element.tag.endswith("}title"):
            continue
        if not element.tag.endswith("}path") or set(element.attrib) - (paint_attributes | {"d", "clip-rule"}):
            raise ValueError(f"Unexpected SVG element in {source}: {element.tag}")
        attrs = svg.attrib | element.attrib
        path = pathops.Path()
        parse_path(attrs["d"], path.getPen(allow_open_paths=True))
        fill = attrs.get("fill", "black")
        stroke = attrs.get("stroke", "none")
        if fill != "none":
            if attrs.get("fill-rule") == "evenodd":
                path.fillType = pathops.FillType.EVEN_ODD
            fill_shape = pathops.simplify(path, clockwise=True)
            layers.append((paint_color(fill, attrs, "fill"), fill_shape.transform(*transform)))
        if stroke != "none":
            if attrs["stroke-linecap"] != "round" or attrs["stroke-linejoin"] != "round":
                raise ValueError(f"Unexpected SVG stroke in {source}")
            # Skia's stroke subdivision uses an absolute coordinate tolerance.
            # Stroke in a large coordinate space before returning to the viewBox.
            stroke_scale = 1536 * scale
            outline = path.transform(stroke_scale, 0, 0, stroke_scale, 0, 0)
            outline.stroke(float(attrs["stroke-width"]) * stroke_scale, pathops.LineCap.ROUND_CAP,
                           pathops.LineJoin.ROUND_JOIN, 4)
            outline.convertConicsToQuads(0.032)
            # Resolve intersections before normalization. At unit scale,
            # PathOps may merge nearly coincident stroke contours.
            outline = pathops.simplify(outline, clockwise=True)
            layers.append((paint_color(stroke, attrs, "stroke"), outline.transform(
                scale / stroke_scale, 0, 0, scale / stroke_scale, transform[4], transform[5],
            )))
    return layers


def paint_color(value, attrs, property_name):
    rgb = ImageColor.getrgb(attrs.get("color", "black") if value == "currentColor" else value)
    opacity = max(0, min(1, float(attrs.get(f"{property_name}-opacity", "1"))))
    return (*rgb, round(opacity * 255))


def composite(color, coverage, image):
    layer = Image.new("RGBA", image.size, color)
    mask = Image.frombytes("L", image.size, coverage)
    if color[3] != 255:
        mask = mask.point([round(value * color[3] / 255) for value in range(256)])
    layer.putalpha(mask)
    image.alpha_composite(layer)


def command_process(executable, size, commands):
    run([str(work / executable), str(size)], input=commands, text=True, cwd=work, check=True)


def blend_image(size, layers):
    commands = "".join(
        f"{a:02X}{r:02X}{g:02X}{b:02X}\n{outline.serialize(size)}\nE\n"
        for (r, g, b, a), outline in layers
    )
    command_process("blend.exe", size, commands)
    with Image.open(work / "blend-output.png") as image:
        return image.convert("RGBA")


def manson_image(size, layers):
    image = Image.new("RGBA", (size, size))
    for color, outline in layers:
        # The upstream scanline code expects every control point inside its target.
        extent = max(max(-coord, coord - 1) for _, points in outline.commands for coord in points)
        pad = max(2, ceil(extent * size) + 2)
        edge = size + pad * 2
        command_process("manson.exe", edge, outline.serialize(size, pad))
        coverage = Image.frombytes("L", (edge, edge), (work / "manson-output.raw").read_bytes())
        composite(color, coverage.crop((pad, pad, pad + size, pad + size)).tobytes(), image)
    return image


def freetype_face(path):
    # Give the icon a Latin code point so FreeType applies its Latin auto-hinter.
    # Arial supplies the reference glyphs used to establish blue zones.
    with TTFont(font_file) as font:
        glyph = font.getBestCmap()[ord("A")]
        em = font["head"].unitsPerEm
        pen = TTGlyphPen(None)
        path.draw(TransformPen(Cu2QuPen(pen, 0.4), (em, 0, 0, -em, 0, em)))
        font["glyf"][glyph] = outline = pen.glyph()
        outline.recalcBounds(font["glyf"])
        font["hmtx"][glyph] = (em, outline.xMin)
        data = BytesIO()
        font.save(data)
    return freetype.Face.from_bytes(data.getvalue())


def freetype_image(size, layers, *, hinting=True):
    image = Image.new("RGBA", (size, size))
    mode = freetype.FT_LOAD_FORCE_AUTOHINT if hinting else freetype.FT_LOAD_NO_HINTING
    for color, face in layers:
        face.set_pixel_sizes(size, size)
        face.load_char("A", freetype.FT_LOAD_RENDER | mode)
        glyph = face.glyph
        bitmap = glyph.bitmap
        pixels = Image.frombytes("L", (bitmap.width, bitmap.rows), bytes(bitmap.buffer),
                                 "raw", "L", bitmap.pitch)
        coverage = Image.new("L", image.size)
        coverage.paste(pixels, (glyph.bitmap_left, size - glyph.bitmap_top))
        composite(color, coverage.tobytes(), image)
    return image


def joint_image(size, layers, grid=16):
    # Approximate joint prefiltering by evaluating the full paint stack at
    # each subpixel before the box filter. This experiment uses a regular grid.
    edge = size * grid
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, edge, edge)
    context = cairo.Context(surface)
    context.scale(edge, edge)
    context.set_antialias(cairo.ANTIALIAS_NONE)
    for color, outline in layers:
        context.set_source_rgba(*(channel / 255 for channel in color))
        x, y = 0, 0
        for command, points in outline.commands:
            if command == "M":
                x, y = points
                context.move_to(x, y)
            elif command == "L":
                x, y = points
                context.line_to(x, y)
            elif command == "Q":
                u, v, end_x, end_y = points
                context.curve_to((x + 2 * u) / 3, (y + 2 * v) / 3,
                                 (end_x + 2 * u) / 3, (end_y + 2 * v) / 3, end_x, end_y)
                x, y = end_x, end_y
            elif command == "C":
                context.curve_to(*points)
                x, y = points[-2:]
            else:
                context.close_path()
                x, y = context.get_current_point()
        context.fill()
    surface.flush()
    bgra = np.frombuffer(surface.get_data(), dtype=np.uint8).reshape(size, grid, size, grid, 4)
    rgba = bgra.mean(axis=(1, 3))[:, :, [2, 1, 0, 3]]
    alpha = rgba[:, :, 3:]
    rgb = np.divide(rgba[:, :, :3] * 255, alpha, out=np.zeros_like(rgba[:, :, :3]), where=alpha > 0)
    return Image.fromarray(np.rint(np.concatenate((rgb, alpha), axis=2)).astype(np.uint8))


gpu_setup = """
async () => {
    const {requestDevice} = await import('/windfoil/src/gpu.js');
    const {pathToQuads} = await import('/windfoil/tools/common/svg-parse.js');
    const {windfoilCoverage, slugCoverage} = await import('/windfoil/tools/common/coverage.js');
    const device = await requestDevice();
    window.renderCoverage = async ({size, paths}) => {
        const methodLayers = [[], []];
        device.pushErrorScope('validation');
        for (const path of paths) {
            const quads = pathToQuads(path, 0.0005);
            for (const [index, raster] of [windfoilCoverage, slugCoverage].entries()) {
                const coverage = await raster(quads, {size, device});
                methodLayers[index].push(Array.from(coverage, value => Math.round(value * 255)));
            }
        }
        const error = await device.popErrorScope();
        if (error) throw new Error(error.message);
        return methodLayers;
    };
}
"""


def gpu_images(page, size, layers):
    paths = [outline.serialize(size) for _, outline in layers]
    methods = page.evaluate("options => renderCoverage(options)", {"size": size, "paths": paths})
    images = []
    for method in methods:
        image = Image.new("RGBA", (size, size))
        for ((color, _), coverage) in zip(layers, method, strict=True):
            composite(color, bytes(coverage), image)
        images.append(image)
    return images


def prepare_sample(source):
    outlines = open_layers(source)
    return ([(color, OutlineCommands(path)) for color, path in outlines],
            [(color, freetype_face(path)) for color, path in outlines])


def render_methods(page, size, layers, glyphs):
    return (
        blend_image(size, layers),
        manson_image(size, layers),
        joint_image(size, layers),
        freetype_image(size, glyphs),
        *gpu_images(page, size, layers),
    )


def generate(page):
    samples = [prepare_sample(source) for source in sources]
    for size in sizes:
        sheet = Image.new("RGBA", (size * len(sources), size * len(methods)))
        for sample, (layers, glyphs) in enumerate(samples):
            images = render_methods(page, size, layers, glyphs)
            for method, image in enumerate(images):
                if not image.getbbox():
                    raise ValueError(f"Empty {size}px output for {sources[sample].name}, method {method}")
                sheet.paste(image, (sample * size, method * size))
        sheet.save(output / f"algorithms-{size}.png", optimize=True)
        print(f"{size}px: {len(sources)} SVGs rendered by all six methods", flush=True)


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


@contextmanager
def shader_page():
    with ThreadingHTTPServer(("127.0.0.1", 0), partial(QuietHandler, directory=work)) as server:
        Thread(target=server.serve_forever, daemon=True).start()
        try:
            with sync_playwright() as playwright:
                browser = playwright.chromium.launch(channel="chrome", args=[
                    "--enable-unsafe-webgpu", "--use-webgpu-adapter=swiftshader", "--enable-unsafe-swiftshader",
                ])
                page = browser.new_page()
                # The glyph-font parser is imported by the benchmark's Slug
                # module, but neither shader uses it for direct path input.
                page.route("**/preview.html", lambda route: route.fulfill(
                    body="""<script type="importmap">{"imports":{"opentype.js":"/unused-font.js"}}</script>""",
                    content_type="text/html",
                ))
                page.route("**/unused-font.js", lambda route: route.fulfill(
                    body="export default {};", content_type="text/javascript",
                ))
                page.goto(f"http://127.0.0.1:{server.server_port}/preview.html")
                page.evaluate(gpu_setup)
                yield page
        finally:
            server.shutdown()


if __name__ == "__main__":
    with shader_page() as page:
        generate(page)
