use super::{
    State,
    paint::{TEXT, color, rect},
};
use windows::{
    Win32::Graphics::{Direct2D::*, DirectWrite::*},
    core::{Error, Interface, PCWSTR, Result, w},
};
use windows_numerics::Vector2;
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::*,
    UI::{
        Controls::{EM_GETRECT, EM_GETSEL, EM_SETCUEBANNER},
        WindowsAndMessaging::*,
    },
};

pub(super) const SET_SAMPLE: u32 = WM_APP + 30;
pub(super) const ACTIVE_RENDERER: u32 = WM_APP + 31;
pub(super) const VERIFY_FONT: u32 = WM_APP + 32;

pub(super) fn solid_window() -> bool {
    std::env::var_os("YISOU_TEXT_RENDER_SOLID").is_some()
}

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Current = 1,
    Gray = 2,
    GridFit = 3,
    Solid = 4,
}

#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub family: &'static str,
    pub size: i32,
    pub mode: Mode,
}

pub(super) fn set_sample(state: &State, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let mode = match wp {
        1 => Mode::Current,
        2 => Mode::Gray,
        3 => Mode::GridFit,
        4 => Mode::Solid,
        _ => return 0,
    };
    let Some(&family) = [
        "SimHei",
        "SimSun",
        "FangSong",
        "KaiTi",
        "Microsoft YaHei UI",
    ]
    .get((lp as usize >> 16) & 0xff) else {
        return 0;
    };
    let size = (lp as i32 >> 8) & 0xff;
    let dpi = lp as u32 & 0xff;
    if ![14, 20, 26].contains(&size) || ![96, 120, 144].contains(&dpi) || state.edit.get().is_null()
    {
        return 0;
    }
    state.text_sample.set(Some(Sample { family, size, mode }));
    state.text_receipt.set(0);
    state.dpi.set(dpi);
    state.layout_dpi.set(0);
    state.backdrop_unavailable.set(false);
    state.apply_font();
    state.layout();
    unsafe {
        SendMessageW(
            state.edit.get(),
            EM_SETCUEBANNER,
            0,
            w!("").as_ptr() as isize,
        );
        InvalidateRect(state.window.get(), std::ptr::null(), 0);
        UpdateWindow(state.window.get());
    }
    1
}

pub(super) fn active_renderer(state: &State) -> LRESULT {
    state.text_receipt.get() as LRESULT
}

pub(super) fn record_draw(state: &State, mode: Mode) {
    if state.text_sample.get().is_some() {
        let count = (state.text_receipt.get() >> 3) + 1;
        state.text_receipt.set((count << 3) | mode as usize);
    }
}

pub(super) fn verify_font(state: &State) -> LRESULT {
    let Some(sample) = state.text_sample.get() else {
        return 0;
    };
    let edit = state.edit.get();
    let text: Vec<u16> = super::text_of(edit).encode_utf16().collect();
    unsafe {
        let font = SendMessageW(edit, WM_GETFONT, 0, 0) as HFONT;
        let mut options = LOGFONTW::default();
        if GetObjectW(
            font,
            size_of::<LOGFONTW>() as i32,
            (&mut options as *mut LOGFONTW).cast(),
        ) == 0
            || options.lfHeight != -state.scale(sample.size)
            || options.lfQuality != CLEARTYPE_QUALITY
        {
            return 0;
        }
        let dc = GetDC(edit);
        let previous = SelectObject(dc, font);
        let mut face = [0; 64];
        let length = GetTextFaceW(dc, face.len() as i32, face.as_mut_ptr());
        let actual = String::from_utf16_lossy(&face[..length.max(0) as usize]);
        let mut glyphs = vec![0; text.len()];
        let complete = text.is_empty()
            || (GetGlyphIndicesW(
                dc,
                text.as_ptr(),
                text.len() as i32,
                glyphs.as_mut_ptr(),
                GGI_MARK_NONEXISTING_GLYPHS,
            ) != GDI_ERROR as u32
                && !glyphs.contains(&0xffff));
        SelectObject(dc, previous);
        ReleaseDC(edit, dc);
        let locale_names = [
            ("SimHei", "黑体"),
            ("SimSun", "宋体"),
            ("FangSong", "仿宋"),
            ("KaiTi", "楷体"),
            ("Microsoft YaHei UI", "微软雅黑 UI"),
        ];
        let actual = actual.trim_end_matches('\0');
        let matches = actual.eq_ignore_ascii_case(sample.family)
            || locale_names.contains(&(sample.family, actual));
        if complete && matches {
            -options.lfHeight as isize
        } else {
            0
        }
    }
}

pub(super) struct DirectEdit {
    factory: IDWriteFactory,
    format: IDWriteTextFormat,
    params: Option<IDWriteRenderingParams>,
}

impl DirectEdit {
    pub fn mode(&self) -> Mode {
        if self.params.is_some() {
            Mode::GridFit
        } else {
            Mode::Gray
        }
    }

    pub fn new(state: &State) -> Result<Option<Self>> {
        let Some(sample) = state.text_sample.get() else {
            return Ok(None);
        };
        if !matches!(sample.mode, Mode::Gray | Mode::GridFit) {
            return Ok(None);
        }
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let family = super::wide(sample.family);
            let mut collection = None;
            factory.GetSystemFontCollection(&mut collection, false)?;
            let collection = collection
                .ok_or_else(|| Error::from_hresult(windows::Win32::Foundation::E_FAIL))?;
            let (mut index, mut exists) = (0, windows::core::BOOL(0));
            collection.FindFamilyName(PCWSTR(family.as_ptr()), &mut index, &mut exists)?;
            if !exists.as_bool() {
                return Err(Error::from_hresult(windows::Win32::Foundation::E_FAIL));
            }
            let face = collection
                .GetFontFamily(index)?
                .GetFirstMatchingFont(
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                )?
                .CreateFontFace()?;
            // The same installed face must cover the samples without font fallback.
            let points: Vec<u32> = "警鑿鬱贛纏競齒讓贏藏驟疆邊緣細線與筆畫交錯"
                .chars()
                .map(u32::from)
                .collect();
            let mut glyphs = vec![0u16; points.len()];
            face.GetGlyphIndices(points.as_ptr(), points.len() as u32, glyphs.as_mut_ptr())?;
            if glyphs.contains(&0) {
                return Err(Error::from_hresult(windows::Win32::Foundation::E_FAIL));
            }
            let format = factory.CreateTextFormat(
                PCWSTR(family.as_ptr()),
                &collection,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                state.scale(sample.size) as f32 * 96.0 / state.dpi.get() as f32,
                w!("zh-CN"),
            )?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let params = if matches!(sample.mode, Mode::GridFit) {
                let defaults = factory.CreateRenderingParams()?;
                let factory2 = factory.cast::<IDWriteFactory2>()?;
                Some(
                    factory2
                        .CreateCustomRenderingParams(
                            defaults.GetGamma(),
                            defaults.GetEnhancedContrast(),
                            1.0,
                            0.0,
                            DWRITE_PIXEL_GEOMETRY_FLAT,
                            DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                            DWRITE_GRID_FIT_MODE_ENABLED,
                        )?
                        .cast()?,
                )
            } else {
                None
            };
            Ok(Some(Self {
                factory,
                format,
                params,
            }))
        }
    }

    pub fn draw(
        &self,
        state: &State,
        canvas: &ID2D1DeviceContext,
        brush: &ID2D1SolidColorBrush,
    ) -> Result<()> {
        let edit = state.edit.get();
        let text: Vec<_> = super::text_of(edit).encode_utf16().collect();
        if text.is_empty() {
            return Ok(());
        }
        unsafe {
            if canvas.GetTextAntialiasMode() != D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE {
                return Err(Error::from_hresult(windows::Win32::Foundation::E_FAIL));
            }
            let mut bounds = RECT::default();
            let mut origin = POINT::default();
            GetClientRect(edit, &mut bounds);
            MapWindowPoints(edit, state.window.get(), &mut origin, 1);
            let dip = 96.0 / state.dpi.get() as f32;
            let width = bounds.right as f32 * dip;
            let height = bounds.bottom as f32 * dip;
            let layout = self
                .factory
                .CreateTextLayout(&text, &self.format, width, height)?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics)?;
            let mut line = [DWRITE_LINE_METRICS::default()];
            layout.GetLineMetrics(Some(&mut line), &mut 0)?;
            let mut selection_end = 0u32;
            SendMessageW(
                edit,
                EM_GETSEL,
                0,
                (&mut selection_end as *mut u32) as isize,
            );
            let left = origin.x as f32 * dip;
            let top = origin.y as f32 * dip;
            // Use the sample's trailing caret position for the right-edge comparison.
            let right_margin = 2.0 * dip;
            let shift = if selection_end as usize == text.len() {
                (metrics.widthIncludingTrailingWhitespace - width + right_margin).max(0.0)
            } else {
                0.0
            };
            let x = ((left + dip - shift) / dip).round() * dip;
            let mut native_rect = RECT::default();
            SendMessageW(edit, EM_GETRECT, 0, &mut native_rect as *mut RECT as isize);
            let dc = GetDC(edit);
            let old_font = SelectObject(dc, state.font.get());
            let mut native_metrics = TEXTMETRICW::default();
            let measured = GetTextMetricsW(dc, &mut native_metrics) != 0;
            SelectObject(dc, old_font);
            ReleaseDC(edit, dc);
            let baseline = if measured {
                (origin.y + native_rect.top + native_metrics.tmAscent) as f32 * dip
            } else {
                top + (height - line[0].height) * 0.5 + line[0].baseline
            };
            let y = baseline - line[0].baseline;
            brush.SetColor(&color(TEXT, 1.0));
            canvas
                .PushAxisAlignedClip(&rect(left, top, width, height), D2D1_ANTIALIAS_MODE_ALIASED);
            let previous = ID2D1RenderTarget::GetTextRenderingParams(canvas).ok();
            if let Some(params) = &self.params {
                ID2D1RenderTarget::SetTextRenderingParams(canvas, params);
                let applied = ID2D1RenderTarget::GetTextRenderingParams(canvas)
                    .and_then(|params| params.cast::<IDWriteRenderingParams2>());
                let verified = applied.is_ok_and(|params| {
                    params.GetGridFitMode() == DWRITE_GRID_FIT_MODE_ENABLED
                        && params.GetGrayscaleEnhancedContrast() == 1.0
                });
                if !verified {
                    canvas.PopAxisAlignedClip();
                    return Err(Error::from_hresult(windows::Win32::Foundation::E_FAIL));
                }
            }
            ID2D1RenderTarget::DrawTextLayout(
                canvas,
                Vector2 { X: x, Y: y },
                &layout,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
            if self.params.is_some() {
                ID2D1RenderTarget::SetTextRenderingParams(canvas, previous.as_ref());
            }
            canvas.PopAxisAlignedClip();
        }
        Ok(())
    }
}
