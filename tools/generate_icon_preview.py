"""Generate the icon comparison PNG sheets from the bundled SVG fixtures."""

from argparse import ArgumentParser
from io import BytesIO
from pathlib import Path

import cairosvg
from PIL import Image


root = Path(__file__).resolve().parents[1]
output = root / "assets/icon-preview"
thesvg = output / "thesvg"
tabler = output / "tabler"
sources = (
    tabler / "search.svg",
    thesvg / "yandex/default.svg",
    thesvg / "youtube/mono.svg",
    thesvg / "youtube/default.svg",
    thesvg / "google/color.svg",
    thesvg / "bilibili/color.svg",
    tabler / "flame.svg",
)


small_sizes = (16, 18, 20, 24, 30, 36, 42, 48, 64, 72)


def generate_png():
    for size in (*small_sizes, 128, 256, 512, 1024):
        for variant, edge in (("native", size), ("large", 256 + size)):
            sheet = Image.new("RGBA", (edge * len(sources), edge))
            for index, source in enumerate(sources):
                rendered = cairosvg.svg2png(url=str(source), output_width=edge, output_height=edge)
                with Image.open(BytesIO(rendered)) as image:
                    sheet.paste(image.convert("RGBA"), (index * edge, 0))
            sheet.save(output / f"{variant}-{size}.png", optimize=True)


def generate_reference():
    import numpy as np

    factor = 64
    for size in small_sizes:
        sheet = Image.new("RGBA", (size * len(sources), size))
        edge = size * factor
        for index, source in enumerate(sources):
            rendered = cairosvg.svg2png(url=str(source), output_width=edge, output_height=edge)
            with Image.open(BytesIO(rendered)) as image:
                pixels = np.asarray(image.convert("RGBA").convert("RGBa"))
                average = pixels.reshape(size, factor, size, factor, 4).mean(axis=(1, 3))
                tile = Image.frombytes("RGBa", (size, size), np.rint(average).astype(np.uint8).tobytes())
                sheet.paste(tile.convert("RGBA"), (index * size, 0))
        sheet.save(output / f"reference-{size}.png", optimize=True)


if __name__ == "__main__":
    parser = ArgumentParser()
    parser.add_argument("--reference", action="store_true")
    args = parser.parse_args()
    output.mkdir(exist_ok=True)
    if args.reference:
        generate_reference()
    else:
        generate_png()
