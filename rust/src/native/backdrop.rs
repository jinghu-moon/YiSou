use super::{
    State,
    edit_paint::EditLayer,
    paint::{Label, MUTED, Painter, SURFACE, color, pixel_format, rect},
    search_text::SearchText,
};
use windows::{
    Win32::{
        Foundation::{E_FAIL, HMODULE, HWND, POINT},
        Graphics::{
            Direct2D::{Common::*, *},
            Direct3D::D3D_DRIVER_TYPE_WARP,
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_CREATE_DEVICE_PREVENT_INTERNAL_THREADING_OPTIMIZATIONS, D3D11_SDK_VERSION,
                D3D11CreateDevice,
            },
            DirectComposition::{
                DCompositionCreateDevice2, IDCompositionDesktopDevice, IDCompositionSurface,
                IDCompositionTarget, IDCompositionVisual2,
            },
            Dxgi::{
                Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM},
                IDXGIDevice, IDXGISurface,
            },
        },
    },
    core::{Error, Interface, Result},
};
use windows_sys::Win32::{
    Foundation::{HWND as RawWindow, RECT},
    Graphics::Dwm::{
        DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
        DWMWA_USE_HOSTBACKDROPBRUSH, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
    },
    UI::{
        Controls::MARGINS,
        WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, SPI_GETHIGHCONTRAST, SetWindowLongPtrW,
            SystemParametersInfoW, WS_EX_NOREDIRECTIONBITMAP,
        },
    },
};

pub(super) struct Backdrop {
    device: IDCompositionDesktopDevice,
    _target: IDCompositionTarget,
    visual: IDCompositionVisual2,
    canvas: ID2D1DeviceContext,
    painter: Painter,
    caret_brush: ID2D1SolidColorBrush,
    edit: Option<EditLayer>,
    search_text: Option<SearchText>,
    search_text_active: Option<bool>,
    #[cfg(feature = "text-render-lab")]
    direct_edit: Option<super::text_render_lab::DirectEdit>,
    surface: Option<(u32, u32, IDCompositionSurface)>,
}

impl Backdrop {
    fn new(state: &State) -> Result<Self> {
        unsafe {
            let mut graphics = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT
                    | D3D11_CREATE_DEVICE_PREVENT_INTERNAL_THREADING_OPTIMIZATIONS,
                None,
                D3D11_SDK_VERSION,
                Some(&mut graphics),
                None,
                None,
            )?;
            let dxgi: IDXGIDevice = graphics
                .ok_or_else(|| Error::from_hresult(E_FAIL))?
                .cast()?;
            let drawing = D2D1CreateDevice(&dxgi, None)?;
            drawing.SetMaximumTextureMemory(512 * 1024);
            let device: IDCompositionDesktopDevice = DCompositionCreateDevice2(&drawing)?;
            let target = device.CreateTargetForHwnd(HWND(state.window.get()), true)?;
            let visual = device.CreateVisual()?;
            target.SetRoot(&visual)?;
            let canvas = drawing.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            canvas.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let (family, collection) = state.drawing_font();
            let painter = Painter::for_target(canvas.clone().into(), &family, collection)?;
            let caret_brush = canvas.CreateSolidColorBrush(&color(0, 1.0), None)?;
            Ok(Self {
                device,
                _target: target,
                visual,
                canvas,
                painter,
                caret_brush,
                edit: None,
                search_text: None,
                search_text_active: None,
                #[cfg(feature = "text-render-lab")]
                direct_edit: super::text_render_lab::DirectEdit::new(state)?,
                surface: None,
            })
        }
    }

    fn paint(&mut self, state: &State, width: u32, height: u32) -> Result<()> {
        unsafe {
            let resized = self
                .surface
                .as_ref()
                .is_none_or(|(w, h, _)| (*w, *h) != (width, height));
            if resized {
                let surface = self.device.CreateSurface(
                    width,
                    height,
                    DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_ALPHA_MODE_PREMULTIPLIED,
                )?;
                self.surface = Some((width, height, surface));
            }
            let (_, _, surface) = self.surface.as_ref().unwrap();
            let mut offset = POINT::default();
            let image: IDXGISurface = surface.BeginDraw(None, &mut offset)?;
            let result = self
                .canvas
                .CreateBitmapFromDxgiSurface(
                    &image,
                    Some(&D2D1_BITMAP_PROPERTIES1 {
                        pixelFormat: pixel_format(D2D1_ALPHA_MODE_PREMULTIPLIED),
                        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                        ..Default::default()
                    }),
                )
                .and_then(|target| {
                    let canvas = &self.canvas;
                    let dpi = state.dpi.get() as f32;
                    let dip = 96.0 / dpi;
                    canvas.SetTarget(&target);
                    canvas.SetDpi(dpi, dpi);
                    let mut transform = Default::default();
                    canvas.GetTransform(&mut transform);
                    transform.M31 = offset.x as f32 * dip;
                    transform.M32 = offset.y as f32 * dip;
                    canvas.SetTransform(&transform);
                    canvas.BeginDraw();
                    canvas.PushAxisAlignedClip(
                        &rect(0.0, 0.0, width as f32 * dip, height as f32 * dip),
                        D2D1_ANTIALIAS_MODE_ALIASED,
                    );
                    canvas.Clear(Some(&color(SURFACE, 0.68)));
                    #[cfg(feature = "text-render-lab")]
                    let edit = if state.text_sample.get().is_some() {
                        self.direct_edit
                            .as_ref()
                            .map(|direct| direct.draw(state, canvas, &self.caret_brush))
                            .unwrap_or_else(|| {
                                EditLayer::draw(&mut self.edit, state, canvas, &self.caret_brush)
                            })
                    } else {
                        Self::draw_search_text(
                            &mut self.search_text,
                            &mut self.search_text_active,
                            &mut self.edit,
                            state,
                            canvas,
                            &self.caret_brush,
                        )
                    };
                    #[cfg(not(feature = "text-render-lab"))]
                    let edit = Self::draw_search_text(
                        &mut self.search_text,
                        &mut self.search_text_active,
                        &mut self.edit,
                        state,
                        canvas,
                        &self.caret_brush,
                    );
                    state.paint_composed_engine(&mut self.painter);
                    self.painter.text(
                        state.hint.get(),
                        rect(20.0, 52.0, 540.0, 24.0),
                        if state.error.get() { 0x9d3026 } else { MUTED },
                        Label::Hint,
                    );
                    canvas.PopAxisAlignedClip();
                    let drawn = canvas.EndDraw(None, None);
                    canvas.SetTarget(None::<&ID2D1Image>);
                    edit.and(drawn)
                });
            drop(image);
            let ended = surface.EndDraw();
            result.and(ended)?;
            if resized {
                self.visual.SetContent(surface)?;
            }
            self.device.Commit()?;
            #[cfg(feature = "text-render-lab")]
            super::text_render_lab::record_draw(
                state,
                self.direct_edit
                    .as_ref()
                    .map_or(super::text_render_lab::Mode::Current, |edit| edit.mode()),
            );
        }
        Ok(())
    }

    fn draw_search_text(
        search_text: &mut Option<SearchText>,
        active: &mut Option<bool>,
        edit: &mut Option<EditLayer>,
        state: &State,
        canvas: &ID2D1DeviceContext,
        brush: &ID2D1SolidColorBrush,
    ) -> Result<()> {
        if !state.composing.get() {
            let first = search_text.is_none();
            let result = (|| {
                if first {
                    *search_text = Some(SearchText::new(state)?);
                }
                search_text.as_mut().unwrap().draw(state, canvas, brush)
            })();
            if result.is_ok() {
                if active.replace(true) != Some(true) {
                    state.diagnostics.sample("search_text_direct");
                }
                return result;
            }
        }
        if active.replace(false) != Some(false) {
            state.diagnostics.sample("search_text_fallback");
            state.edit_dirty.set(true);
        }
        // The native Edit still owns IME composition and provides the fallback.
        EditLayer::draw(edit, state, canvas, brush)
    }
}

fn set_material(hwnd: RawWindow, enable: bool) -> bool {
    let host = i32::from(enable);
    let material = if enable {
        DWMSBT_TRANSIENTWINDOW
    } else {
        DWMSBT_NONE
    };
    let margin = if enable { -1 } else { 0 };
    let margins = MARGINS {
        cxLeftWidth: margin,
        cxRightWidth: margin,
        cyTopHeight: margin,
        cyBottomHeight: margin,
    };
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH as u32,
            (&host as *const i32).cast(),
            4,
        );
        let backdrop = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            (&material as *const i32).cast(),
            4,
        );
        let frame = DwmExtendFrameIntoClientArea(hwnd, &margins);
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let flag = WS_EX_NOREDIRECTIONBITMAP as isize;
        let use_composition = enable && backdrop >= 0 && frame >= 0;
        let updated = if use_composition {
            style | flag
        } else {
            style & !flag
        };
        if updated != style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, updated);
        }
        backdrop >= 0 && frame >= 0
    }
}

pub(super) fn high_contrast() -> bool {
    #[repr(C)]
    struct HighContrast {
        size: u32,
        flags: u32,
        scheme: *mut u16,
    }
    let mut settings = HighContrast {
        size: size_of::<HighContrast>() as u32,
        flags: 0,
        scheme: std::ptr::null_mut(),
    };
    unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            settings.size,
            (&mut settings as *mut HighContrast).cast(),
            0,
        ) != 0
            && settings.flags & 1 != 0
    }
}

impl State {
    pub(super) fn prepare_backdrop(&self) {
        if self.backdrop_unavailable.get() || self.composing_controls.get() {
            return;
        }
        let hwnd = self.window.get();
        #[cfg(feature = "text-render-lab")]
        let force_solid = super::text_render_lab::solid_window();
        #[cfg(not(feature = "text-render-lab"))]
        let force_solid = false;
        if force_solid || high_contrast() || !set_material(hwnd, true) {
            set_material(hwnd, false);
            self.backdrop_unavailable.set(true);
            self.diagnostics.sample("backdrop_solid");
        }
    }

    pub(super) fn paint_backdrop(&self, bounds: &RECT) -> bool {
        if bounds.right <= 0 || bounds.bottom <= 0 || self.backdrop_unavailable.get() {
            return false;
        }
        self.prepare_backdrop();
        if self.backdrop_unavailable.get() {
            return false;
        }
        let hwnd = self.window.get();
        let Ok(mut backdrop) = self.backdrop.try_borrow_mut() else {
            return false;
        };
        let newly_created = backdrop.is_none();
        if backdrop.is_none() {
            *backdrop = Backdrop::new(self).ok();
            if backdrop.is_none() {
                set_material(hwnd, false);
                self.backdrop_unavailable.set(true);
                self.diagnostics.sample("backdrop_solid");
                return false;
            }
        }
        if backdrop.as_mut().is_some_and(|b| {
            b.paint(self, bounds.right as u32, bounds.bottom as u32)
                .is_ok()
        }) {
            self.use_composed_controls(true);
            if newly_created {
                self.diagnostics.sample("backdrop_acrylic");
            }
            return true;
        }
        *backdrop = None;
        self.use_composed_controls(false);
        set_material(hwnd, false);
        self.backdrop_unavailable.set(true);
        self.diagnostics.sample("backdrop_solid");
        false
    }

    pub(super) fn release_backdrop(&self) {
        if self
            .backdrop
            .try_borrow_mut()
            .is_ok_and(|mut backdrop| backdrop.take().is_some())
        {
            self.use_composed_controls(false);
        }
    }

    pub(super) fn reset_backdrop_images(&self) {
        if let Some(backdrop) = self.backdrop.borrow_mut().as_mut() {
            backdrop.painter.reset_images();
        }
    }
}
