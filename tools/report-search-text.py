"""Build screenshot sheets and pixel statistics. Requires Pillow and NumPy."""

import argparse
import csv
from html import escape
from itertools import product
from pathlib import Path
from statistics import median

import numpy as np
from PIL import Image, ImageDraw, ImageFont


MODES = {"current": "当前 Acrylic", "gray": "DirectWrite 灰度",
         "grid-fit": "DirectWrite 网格拟合", "solid": "实色 ClearType"}
FONTS = {"SimHei": "黑体", "SimSun": "宋体", "FangSong": "仿宋",
         "KaiTi": "楷体", "Microsoft YaHei UI": "微软雅黑 UI"}
SIZES = [14, 20, 26]
DPIS = [96, 120, 144]


def pixels_at_edit(image, entry):
    left, top, width, height = (int(entry[key]) for key in (
        "edit_x", "edit_y", "edit_width", "edit_height",
    ))
    return image.crop((left, top, left + width, top + height))


def coverage(image, blank):
    rgb = np.asarray(image, dtype=np.float32)
    # Sample the empty line in the same capture region. The top and bottom
    # background can differ slightly, especially with Acrylic.
    backdrop = np.median(np.asarray(blank, dtype=np.float32), axis=1, keepdims=True)
    foreground = np.array([26.0, 32.0, 41.0], dtype=np.float32)
    alpha = (backdrop - rgb) / np.maximum(1.0, backdrop - foreground)
    return alpha.mean(axis=2), 255.0 * np.ptp(alpha, axis=2)


def measure(text, edge, blank, size_px):
    normal_alpha, chroma = coverage(text, blank)
    edge_alpha, _ = coverage(edge, blank)
    color_sample = chroma[(normal_alpha >= 0.08) & (normal_alpha <= 0.85)]

    def last_ink(alpha):
        ink = np.flatnonzero(np.any(alpha >= 0.27, axis=0))
        if len(ink) < 8 * size_px:
            raise ValueError("Too little ink for the expected text")
        return int(ink[-1]) + 1

    end = last_ink(normal_alpha)
    edge_end = last_ink(edge_alpha)
    width = int(round(size_px))
    glyph = normal_alpha[:, end - width:end] >= 0.27
    edge_glyph = edge_alpha[:, edge_end - width:edge_end] >= 0.27
    overlap = (glyph & edge_glyph).sum() / (glyph | edge_glyph).sum()
    return {
        "rgb_edge_p90": round(float(np.percentile(color_sample, 90)), 2)
        if len(color_sample) >= 25 else None,
        "partial_coverage_pixels": len(color_sample),
        "edge_last_glyph_iou": round(float(overlap), 3),
        "edge_last_ink_margin_px": edge.width - edge_end,
    }


def read_capture_rows(directory, native_dpi=None):
    rows = {}
    for name in ("captures.csv", "captures-solid.csv"):
        with (directory / name).open(encoding="utf-8", newline="") as source:
            for row in csv.DictReader(source):
                key = (row["mode"], row["font"], int(row["dip"]), int(row["render_dpi"]), row["state"])
                rows[key] = row
    expected = 4 * 5 * 3 * (1 if native_dpi else 3) * 3
    if len(rows) != expected:
        raise ValueError(f"Expected {expected} captures, got {len(rows)}")
    return rows


def make_report(directory):
    rows = read_capture_rows(directory)
    font_file = Path("C:/Windows/Fonts/msyh.ttc")
    heading = ImageFont.truetype(font_file, 18)
    label = ImageFont.truetype(font_file, 15)
    measurements = []
    links = []
    for size, dpi in product(SIZES, DPIS):
        px = size * dpi / 96
        rounded_px = (size * dpi + 48) // 96
        num_chars = 4
        zoom = 2
        crop_width = int(round(px * num_chars)) + 5
        col_step = max(204, zoom * crop_width + 25)
        row_step = 2 * ((32 * dpi + 48) // 96) * zoom + 40
        sheet = Image.new("RGB", (105 + col_step * 4, 68 + row_step * 5), "#eef1f4")
        draw = ImageDraw.Draw(sheet)
        draw.text((12, 10), f"{size} DIP  ·  {dpi} DPI  ·  {px:g} px/em  ·  2×", font=heading, fill="#1a2029")
        for col, (mode, name) in enumerate(MODES.items()):
            x = 105 + col_step * col
            draw.text((x, 42), name, font=label, fill="#1a2029")
            for i, (font, name) in enumerate(FONTS.items()):
                y = 68 + row_step * i
                if col == 0:
                    draw.text((12, y + 7), name, font=label, fill="#1a2029")
                    draw.text((12, y + 28), "句首 / 右边缘", font=label, fill="#475569")
                scenes = {}
                for state in ("empty", "text", "edge"):
                    row = rows[mode, font, size, dpi, state]
                    with Image.open(directory / row["file"]) as image:
                        scenes[state] = pixels_at_edit(image, row)
                text, edge, blank = (scenes[k] for k in ("text", "edge", "empty"))
                for number, scene in enumerate((text, edge)):
                    left = 0 if number == 0 else scene.width - crop_width
                    crop = scene.crop((left, 0, left + crop_width, scene.height))
                    sheet.paste(crop.resize((crop.width * zoom, crop.height * zoom),
                                            Image.Resampling.NEAREST),
                                (x, y + number * (scene.height * zoom + 4)))
                values = measure(text, edge, blank, rounded_px)
                measurements.append({"dip": size, "dpi": dpi, "font": font, "mode": mode, **values})
        name = f"compare-{size}dip-{dpi}dpi.png"
        sheet.save(directory / name)
        links.append((f"{size} DIP · {dpi} DPI", name))
    with (directory / "metrics.csv").open("w", encoding="utf-8", newline="") as target:
        writer = csv.DictWriter(target, list(measurements[0]))
        writer.writeheader()
        writer.writerows(measurements)
    for size in SIZES:
        for mode in MODES:
            samples = [r for r in measurements if r["dip"] == size and r["mode"] == mode]
            print(size, mode, *(median(r[k] for r in samples if r[k] is not None) for k in (
                "rgb_edge_p90", "edge_last_glyph_iou", "edge_last_ink_margin_px",
            )))
    body = "".join(
        f'<li><a href="{name}">{escape(title)}</a></li>' for title, name in links
    )
    originals = "".join(
        f'<li>{FONTS[font]} · {size} DIP · {dpi} DPI — ' + ' · '.join(
            f'<a href="{mode}-{font}-{size}-dpi{dpi}-{state}.png">{name} {label}</a>'
            for mode, name in MODES.items() for state, label in (("text", "短句"), ("edge", "右缘"))
        ) + '</li>' for size, dpi, font in product(SIZES, DPIS, FONTS)
    )
    (directory / "index.html").write_text(f"""<!doctype html>
<html lang="zh-CN"><meta charset="utf-8"><title>易搜 · 中文渲染对照</title>
<style>body{{margin:2rem;font:16px/1.7 "Microsoft YaHei UI",sans-serif;color:#1a2029}}
a{{color:#005fb8}} h1,h2{{line-height:1.3}} li{{margin:.35em 0}}</style>
<h1>易搜 · 中文渲染对照</h1>
<h2>局部像素对照</h2><ul>{body}</ul>
<p><a href="metrics.csv">像素统计 CSV</a></p>
<h2>原始截图</h2><ul>{originals}</ul></html>""", encoding="utf-8")


def audited_sheet(directory, dpi):
    rows = read_capture_rows(directory, dpi)
    for (mode, _, size, render_dpi, _), entry in rows.items():
        if (render_dpi != dpi or int(entry["monitor_dpi"]) != dpi
                or int(entry["font_px"]) != (size * dpi + 48) // 96
                or int(entry["paint_receipt"]) & 7 != list(MODES).index(mode) + 1
                or int(entry["windows_font_smoothing"]) != 2
                or not 0 <= int(entry["screen_crosscheck_max_rgb_delta"]) <= 4):
            raise ValueError(f"Audit failed for {entry['file']}")

    display_order = (20, 14, 26)
    label_path = Path("C:/Windows/Fonts/simhei.ttf")
    body = ImageFont.truetype(label_path, 17)
    title = ImageFont.truetype(label_path, 25)
    width, gap, left = 282, 12, 133
    edit_height = int(rows["current", "SimHei", 20, dpi, "text"]["edit_height"])
    row_step = edit_height * 3 + 8
    group_step = row_step * len(FONTS) + 43
    sheet = Image.new("RGB", (left + (width + gap) * 4, 100 + group_step * 3),
                      "#f3f5f7")
    draw = ImageDraw.Draw(sheet)
    draw.text((16, 12), f"易搜中文渲染对照 · 实际 {dpi * 100 / 96:g}% 缩放",
              font=title, fill="#171d26")
    draw.text((16, 49), "每组上行是句首原尺寸，下行是输入框右边缘 2× 放大。",
              font=body, fill="#4d5968")
    for col, name in enumerate(MODES.values()):
        draw.text((left + col * (width + gap), 80), name, font=body, fill="#171d26")
    pasted = []
    for section, size in enumerate(display_order):
        y = 110 + group_step * section
        draw.text((16, y), f"{size} DIP", font=title, fill="#171d26")
        for i, (font, name) in enumerate(FONTS.items()):
            y = 146 + group_step * section + row_step * i
            draw.text((16, y + 10), name, font=body, fill="#171d26")
            for col, mode in enumerate(MODES):
                x = left + col * (width + gap)
                for state, zoom in (("text", 1), ("edge", 2)):
                    row = rows[mode, font, size, dpi, state]
                    with Image.open(directory / row["file"]) as image:
                        source = pixels_at_edit(image, row)
                    crop_width = width // zoom
                    crop_left = 0 if state == "text" else source.width - crop_width
                    crop = source.crop((crop_left, 0, crop_left + crop_width, source.height))
                    crop = crop.resize((width, source.height * zoom), Image.Resampling.NEAREST)
                    top = y if state == "text" else y + source.height + 3
                    sheet.paste(crop, (x, top))
                    pasted.append((x, top, crop))
    path = directory / "中文渲染四方案.png"
    sheet.save(path)
    with Image.open(path) as saved:
        for x, y, panel in pasted:
            if saved.crop((x, y, x + panel.width, y + panel.height)).tobytes() != panel.tobytes():
                raise ValueError("PNG pixels differ from the source panels")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--audited-dpi", type=int)
    args = parser.parse_args()
    if args.audited_dpi:
        audited_sheet(args.directory, args.audited_dpi)
    else:
        make_report(args.directory)
