"""Capture YiSou's native renderer matrix. Requires Pillow on Windows."""

import argparse
import csv
import ctypes as c
from ctypes import wintypes as w
from itertools import product
from pathlib import Path
import os
import subprocess
import tempfile
import time

from PIL import ImageGrab
import numpy as np


def bind(dll, name, result, *arguments):
    function = getattr(dll, name)
    function.restype = result
    function.argtypes = arguments
    return function


user = c.WinDLL("user32", use_last_error=True)
dwm = c.WinDLL("dwmapi")
gdi = c.WinDLL("gdi32")
find_window = bind(user, "FindWindowW", w.HWND, w.LPCWSTR, w.LPCWSTR)
get_dlg_item = bind(user, "GetDlgItem", w.HWND, w.HWND, c.c_int)
get_window_rect = bind(user, "GetWindowRect", w.BOOL, w.HWND, c.POINTER(w.RECT))
send_message = bind(user, "SendMessageW", c.c_ssize_t, w.HWND, w.UINT, w.WPARAM, w.LPARAM)
is_visible = bind(user, "IsWindowVisible", w.BOOL, w.HWND)
dpi_for_window = bind(user, "GetDpiForWindow", w.UINT, w.HWND)
set_thread_dpi = bind(user, "SetThreadDpiAwarenessContext", w.HANDLE, w.HANDLE)
invalidate = bind(user, "InvalidateRect", w.BOOL, w.HWND, c.POINTER(w.RECT), w.BOOL)
update_window = bind(user, "UpdateWindow", w.BOOL, w.HWND)
get_cursor = bind(user, "GetCursorPos", w.BOOL, c.POINTER(w.POINT))
set_cursor = bind(user, "SetCursorPos", w.BOOL, c.c_int, c.c_int)
get_window_thread = bind(user, "GetWindowThreadProcessId", w.DWORD, w.HWND, c.POINTER(w.DWORD))
window_from_point = bind(user, "WindowFromPoint", w.HWND, w.POINT)
get_ancestor = bind(user, "GetAncestor", w.HWND, w.HWND, w.UINT)
get_dc = bind(user, "GetDC", w.HDC, w.HWND)
release_dc = bind(user, "ReleaseDC", c.c_int, w.HWND, w.HDC)
get_pixel = bind(gdi, "GetPixel", w.DWORD, w.HDC, c.c_int, c.c_int)
system_parameters = bind(user, "SystemParametersInfoW", w.BOOL, w.UINT, w.UINT, w.LPVOID, w.UINT)

MODES = ["current", "gray", "grid-fit", "solid"]
FONTS = ["SimHei", "SimSun", "FangSong", "KaiTi", "Microsoft YaHei UI"]
SIZES = [14, 20, 26]
DPIS = [96, 120, 144]
TEXT = "警鑿鬱贛纏競齒讓贏藏驟疆"
LONG = "邊緣細線與筆畫交錯" * 5 + TEXT


def rect(hwnd):
    bounds = w.RECT()
    if not get_window_rect(hwnd, c.byref(bounds)):
        raise c.WinError(c.get_last_error())
    return bounds.left, bounds.top, bounds.right, bounds.bottom


def set_text(edit, value):
    buffer = c.create_unicode_buffer(value)
    if not send_message(edit, 0x000C, 0, c.addressof(buffer)):
        raise RuntimeError("Cannot set search text")
    returned = c.create_unicode_buffer(len(value) + 1)
    send_message(edit, 0x000D, len(returned), c.addressof(returned))
    if returned.value != value:
        raise RuntimeError("The Edit control returned a different query")


def wait_for_window(process):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline and process.poll() is None:
        window = find_window("YiSou.Rust.Search", None)
        if window:
            owner = w.DWORD()
            get_window_thread(window, c.byref(owner))
            if owner.value != process.pid:
                raise RuntimeError("Close the running YiSou before benchmarking")
            edit = get_dlg_item(window, 100)
            if is_visible(window) and edit:
                return window, edit
        time.sleep(0.025)
    raise RuntimeError("Search window did not open")


def screenshot(window, edit, mode):
    before = send_message(window, 0x801F, 0, 0)
    for hwnd in (edit, window):
        invalidate(hwnd, None, False)
        update_window(hwnd)
    receipt = send_message(window, 0x801F, 0, 0)
    if receipt & 7 != mode or receipt >> 3 <= before >> 3:
        raise RuntimeError(f"No new paint receipt for renderer {mode}: {before} → {receipt}")
    l, t, r, b = rect(window)
    el, et, er, eb = rect(edit)
    for x, y in ((el + 2, (et + eb) // 2), (er - 2, (et + eb) // 2),
                 ((l + r) // 2, t + 5)):
        candidate = window_from_point(w.POINT(x, y))
        if candidate != window and get_ancestor(candidate, 2) != window:
            raise RuntimeError("Another window covers YiSou")
    previous = None
    for _ in range(12):
        if dwm.DwmFlush() != 0:
            raise RuntimeError("DwmFlush failed")
        image = ImageGrab.grab((l, t, r, b), all_screens=True, include_layered_windows=True).convert("RGB")
        edit_bytes = image.crop((el - l, et - t, er - l, eb - t)).tobytes()
        if edit_bytes == previous:
            break
        previous = edit_bytes
        time.sleep(0.025)
    else:
        raise RuntimeError("Edit pixels did not settle")
    screen = get_dc(None)
    try:
        region = np.asarray(image.crop((el - l, et - t, er - l, eb - t)))
        brightness = region.astype(np.int16).mean(axis=2)
        samples = [(el + x, et + y) for y in (3, eb - et - 4)
                   for x in (3, (er - el) // 2, er - el - 4)]
        for mask in (brightness < 90, (brightness >= 90) & (brightness < 160)):
            positions = np.argwhere(mask)
            if len(positions):
                for y, x in positions[::max(1, len(positions) // 5)][:5]:
                    samples.append((el + int(x), et + int(y)))
        max_delta = 0
        for x, y in samples:
            native = get_pixel(screen, x, y)
            rgb = tuple((native >> shift) & 255 for shift in (0, 8, 16))
            captured = image.getpixel((x - l, y - t))
            delta = max(abs(a - b) for a, b in zip(rgb, captured))
            max_delta = max(max_delta, delta)
            if native == 0xffffffff or delta > 4:
                raise RuntimeError(
                    f"Renderer {mode} screen sample at {(x, y)}: GDI {rgb}, Pillow {captured}"
                )
    finally:
        release_dc(None, screen)
    return image, receipt, max_delta


def collect(binary, out, smoke, solid=False, native_dpi=False):
    if find_window("YiSou.Rust.Search", None):
        raise RuntimeError("Close the running YiSou before benchmarking")
    set_thread_dpi(c.c_void_p(-4))
    pointer = w.POINT()
    get_cursor(c.byref(pointer))
    set_cursor(24, 24)
    out.mkdir(parents=True, exist_ok=True)
    fields = ["mode", "font", "dip", "render_dpi", "monitor_dpi", "state", "file",
              "edit_x", "edit_y", "edit_width", "edit_height", "font_px", "paint_receipt",
              "windows_font_smoothing", "screen_crosscheck_max_rgb_delta"]
    modes = ["solid"] if solid else MODES[:-1]
    environment = {**os.environ}
    if solid:
        environment["YISOU_TEXT_RENDER_SOLID"] = "1"
    else:
        environment.pop("YISOU_TEXT_RENDER_SOLID", None)
    log_name = "captures-solid.csv" if solid else "captures.csv"
    try:
        with tempfile.TemporaryDirectory(prefix="yisou-text-") as directory, (
            out / log_name
        ).open("w", newline="", encoding="utf-8") as log:
            config = Path(directory) / "settings.json"
            process = subprocess.Popen(
                [str(binary), "--config", str(config)],
                env={**environment, "LOCALAPPDATA": directory},
            )
            try:
                window, edit = wait_for_window(process)
                monitor_dpi = dpi_for_window(window)
                smoothing = w.UINT()
                if not system_parameters(0x200A, 0, c.byref(smoothing), 0) or smoothing.value != 2:
                    raise RuntimeError("Windows ClearType must be enabled for the opaque control")
                writer = csv.DictWriter(log, fields)
                writer.writeheader()
                cases = product(modes, FONTS, SIZES, [monitor_dpi] if native_dpi else DPIS)
                if smoke:
                    cases = ((mode, "SimSun", 20, 144) for mode in modes)
                for mode, font, size, dpi in cases:
                    mode_id = MODES.index(mode) + 1
                    packed = (FONTS.index(font) << 16) | (size << 8) | dpi
                    c.set_last_error(0)
                    result = send_message(window, 0x801E, mode_id, packed)
                    if result != 1:
                        raise RuntimeError(
                            f"Cannot set {mode}, {font}, {size}, {dpi}: "
                            f"return code {result}, Win32 error {c.get_last_error()}"
                        )
                    for scene, value in (("empty", ""), ("text", TEXT), ("edge", LONG)):
                        set_text(edit, value)
                        # Set the native caret and allow the Edit control to scroll at the end.
                        send_message(get_dlg_item(window, 101), 0x0100, 0x09, 0)
                        send_message(edit, 0x00B1, len(value), len(value))
                        send_message(edit, 0x0100, 0x23, 0)
                        send_message(edit, 0x0100, 0x09, 0)
                        font_px = send_message(window, 0x8020, 0, 0)
                        if font_px != (size * dpi + 48) // 96:
                            raise RuntimeError(f"Font or glyph mismatch for {mode}, {font}, {size}, {dpi}")
                        image, receipt, sample_delta = screenshot(window, edit, mode_id)
                        x, y, _, _ = rect(window)
                        l, t, r, b = rect(edit)
                        name = f"{mode}-{font}-{size}-dpi{dpi}-{scene}.png"
                        image.save(out / name)
                        writer.writerow(dict(zip(fields, (
                            mode, font, size, dpi, monitor_dpi, scene, name,
                            l - x, t - y, r - l, b - t, font_px, receipt, smoothing.value, sample_delta,
                        ))))
                    log.flush()
            finally:
                process.terminate()
                process.wait()
    finally:
        set_cursor(pointer.x, pointer.y)


if __name__ == "__main__":
    arguments = argparse.ArgumentParser(description=__doc__)
    arguments.add_argument("binary", type=Path)
    arguments.add_argument("output", type=Path)
    arguments.add_argument("--smoke", action="store_true")
    arguments.add_argument("--native-dpi", action="store_true")
    args = arguments.parse_args()
    for solid in (False, True):
        collect(args.binary.resolve(strict=True), args.output, args.smoke, solid, args.native_dpi)
