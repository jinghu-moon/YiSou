"""Generate the icon comparison PNG sheets from the bundled SVG fixtures."""

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


if __name__ == "__main__":
    output.mkdir(exist_ok=True)
    for size in (16, 18, 20, 24, 30, 36, 42, 48, 64, 72, 128, 256, 512, 1024):
        for variant, edge in (("native", size), ("large", 256 + size)):
            sheet = Image.new("RGBA", (edge * len(sources), edge))
            for index, source in enumerate(sources):
                rendered = cairosvg.svg2png(url=str(source), output_width=edge, output_height=edge)
                with Image.open(BytesIO(rendered)) as image:
                    sheet.paste(image.convert("RGBA"), (index * edge, 0))
            sheet.save(output / f"{variant}-{size}.png", optimize=True)
