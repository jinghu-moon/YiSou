use super::{
    State,
    edit_paint::EditLayer,
    paint::{Label, Painter, color, pixel_format, rect},
    search_text::SearchText,
};
use windows::{
    Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat},
    UI::{
        Color,
        Composition::{
            CompositionColorBrush, CompositionGraphicsDevice, CompositionPathGeometry, Compositor,
            ContainerVisual, Desktop::DesktopWindowTarget, SpriteVisual,
        },
    },
    Win32::{
        Foundation::{E_FAIL, HMODULE, HWND, POINT, SIZE},
        Graphics::{
            Direct2D::{Common::*, *},
            Direct3D::D3D_DRIVER_TYPE_WARP,
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_CREATE_DEVICE_PREVENT_INTERNAL_THREADING_OPTIMIZATIONS, D3D11_SDK_VERSION,
                D3D11CreateDevice,
            },
            Dxgi::{IDXGIDevice, IDXGISurface},
        },
        System::WinRT::Composition::{
            ICompositionDrawingSurfaceInterop, ICompositorDesktopInterop, ICompositorInterop,
        },
    },
    core::{Error, Interface, Result},
};
use windows_numerics::{Vector2, Vector3};
use windows_sys::Win32::{
    Foundation::{HWND as RawWindow, RECT},
    Graphics::Dwm::{
        DWMSBT_NONE, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_HOSTBACKDROPBRUSH,
        DWMWA_USE_IMMERSIVE_DARK_MODE, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
    },
    UI::{
        Controls::MARGINS,
        WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW},
    },
};
use yisou::settings::SearchMaterial;

pub(super) struct CompositionDevice {
    drawing: ID2D1Device,
    graphics: CompositionGraphicsDevice,
    compositor: Compositor,
}

impl CompositionDevice {
    fn new() -> Result<Self> {
        super::composition_queue::prepare()?;
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
            let compositor = Compositor::new()?;
            let graphics = compositor
                .cast::<ICompositorInterop>()?
                .CreateGraphicsDevice(&drawing)?;
            Ok(Self {
                drawing,
                graphics,
                compositor,
            })
        }
    }
}

pub(super) struct Composition {
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    canvas: ID2D1DeviceContext,
    painter: Painter,
    surface: ICompositionDrawingSurfaceInterop,
    clip: CompositionPathGeometry,
    mica_tone: Option<CompositionColorBrush>,
    layout: Option<(u32, u32, super::smooth_corner::CornerShape)>,
    dark: Option<bool>,
}

impl Composition {
    pub(super) fn new(state: &State, window: RawWindow, material: SearchMaterial) -> Result<Self> {
        let mut shared = state.composition_device.borrow_mut();
        if shared.is_none() {
            *shared = Some(CompositionDevice::new()?);
        }
        let CompositionDevice {
            drawing,
            graphics,
            compositor,
        } = shared.as_ref().unwrap();
        let interop = compositor.cast::<ICompositorDesktopInterop>()?;
        let target = unsafe { interop.CreateDesktopWindowTarget(HWND(window), true)? };
        let root = compositor.CreateContainerVisual()?;
        let children = root.Children()?;
        let clip = compositor.CreatePathGeometry()?;
        root.SetClip(&compositor.CreateGeometricClipWithGeometry(&clip)?)?;
        let brush = match material {
            SearchMaterial::Acrylic => Some(compositor.CreateHostBackdropBrush()?),
            SearchMaterial::Mica => Some(compositor.TryCreateBlurredWallpaperBackdropBrush()?),
            SearchMaterial::Solid => None,
        };
        if let Some(brush) = brush {
            let backdrop = compositor.CreateSpriteVisual()?;
            backdrop.SetBrush(&brush)?;
            backdrop.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
            children.InsertAtTop(&backdrop)?;
        }
        let mica_tone = if material == SearchMaterial::Mica {
            let tone = compositor.CreateColorBrush()?;
            let layer = compositor.CreateSpriteVisual()?;
            layer.SetBrush(&tone)?;
            layer.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
            children.InsertAtTop(&layer)?;
            Some(tone)
        } else {
            None
        };

        let drawing_surface = graphics.CreateDrawingSurface2(
            windows::Graphics::SizeInt32 {
                Width: 0,
                Height: 0,
            },
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            DirectXAlphaMode::Premultiplied,
        )?;
        let surface = drawing_surface.cast()?;
        let foreground = compositor.CreateSpriteVisual()?;
        foreground.SetBrush(&compositor.CreateSurfaceBrushWithSurface(&drawing_surface)?)?;
        foreground.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        children.InsertAtTop(&foreground)?;
        target.SetRoot(&root)?;

        let canvas = unsafe {
            let canvas = drawing.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            canvas.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            canvas
        };
        let (family, collection) = state.drawing_font();
        let painter = Painter::for_target(canvas.clone().into(), &family, collection)?;
        Ok(Self {
            _target: target,
            root,
            canvas,
            painter,
            surface,
            clip,
            mica_tone,
            layout: None,
            dark: None,
        })
    }

    pub(super) fn paint(
        &mut self,
        (width, height): (u32, u32),
        dpi: u32,
        radius: i32,
        dark: bool,
        draw: impl FnOnce(&mut Painter, &ID2D1DeviceContext) -> Result<()>,
    ) -> Result<()> {
        let ratio = dpi as f32 / 96.0;
        let corner = super::smooth_corner::CornerShape::new(
            radius as f32 * ratio,
            width as f32,
            height as f32,
        );
        let layout = (width, height, corner);
        if self.layout != Some(layout) {
            if self
                .layout
                .is_none_or(|(w, h, _)| (w, h) != (width, height))
            {
                unsafe {
                    self.surface.Resize(SIZE {
                        cx: width as i32,
                        cy: height as i32,
                    })?
                };
            }
            let size = Vector2 {
                X: width as f32,
                Y: height as f32,
            };
            self.root.SetSize(size)?;
            let factory = unsafe { self.canvas.GetFactory()? };
            self.clip.SetPath(&super::smooth_corner::composition_path(
                factory,
                rect(0.0, 0.0, size.X, size.Y),
                corner,
            )?)?;
            self.layout = Some(layout);
        }
        if self.dark != Some(dark)
            && let Some(tone) = &self.mica_tone
        {
            // The wallpaper brush is theme-neutral. Apply the selected Mica tone before user tint.
            let value = if dark { 32 } else { 243 };
            tone.SetColor(Color {
                A: 192,
                R: value,
                G: value,
                B: value,
            })?;
            self.dark = Some(dark);
        }
        unsafe {
            let mut offset = POINT::default();
            let image: IDXGISurface = self.surface.BeginDraw(None, &mut offset)?;
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
                    canvas.SetTarget(&target);
                    canvas.SetDpi(dpi as f32, dpi as f32);
                    let mut transform = Default::default();
                    canvas.GetTransform(&mut transform);
                    transform.M31 = offset.x as f32 / ratio;
                    transform.M32 = offset.y as f32 / ratio;
                    canvas.SetTransform(&transform);
                    canvas.BeginDraw();
                    canvas.PushAxisAlignedClip(
                        &rect(0.0, 0.0, width as f32 / ratio, height as f32 / ratio),
                        D2D1_ANTIALIAS_MODE_ALIASED,
                    );
                    let result = draw(&mut self.painter, canvas);
                    canvas.PopAxisAlignedClip();
                    let drawn = canvas.EndDraw(None, None);
                    canvas.SetTarget(None::<&ID2D1Image>);
                    result.and(drawn)
                });
            drop(image);
            let ended = self.surface.EndDraw();
            result.and(ended)?;
        }
        Ok(())
    }
}

pub(super) struct Backdrop {
    selected: SearchMaterial,
    composition: Composition,
    text_brush: ID2D1SolidColorBrush,
    caret: Caret,
    edit: Option<EditLayer>,
    search_text: Option<SearchText>,
    search_text_active: Option<bool>,
    #[cfg(feature = "text-render-lab")]
    direct_edit: Option<super::text_render_lab::DirectEdit>,
}

struct Caret {
    visual: SpriteVisual,
    brush: CompositionColorBrush,
    positioned: bool,
}

impl Caret {
    fn new(root: &ContainerVisual) -> Result<Self> {
        let compositor = root.Compositor()?;
        let visual = compositor.CreateSpriteVisual()?;
        let brush = compositor.CreateColorBrush()?;
        visual.SetBrush(&brush)?;
        visual.SetIsVisible(false)?;
        root.Children()?.InsertAtTop(&visual)?;
        Ok(Self {
            visual,
            brush,
            positioned: false,
        })
    }

    fn update(
        &mut self,
        bounds: Option<D2D_RECT_F>,
        dpi: u32,
        rgb: u32,
        visible: bool,
    ) -> Result<()> {
        self.positioned = bounds.is_some();
        if let Some(bounds) = bounds {
            let scale = dpi as f32 / 96.0;
            self.visual.SetOffset(Vector3 {
                X: (bounds.left * scale).round(),
                Y: (bounds.top * scale).round(),
                Z: 0.0,
            })?;
            self.visual.SetSize(Vector2 {
                X: ((bounds.right - bounds.left) * scale).round(),
                Y: ((bounds.bottom - bounds.top) * scale).round(),
            })?;
            let [_, r, g, b] = rgb.to_be_bytes();
            self.brush.SetColor(Color {
                A: 255,
                R: r,
                G: g,
                B: b,
            })?;
        }
        self.show(visible)
    }

    fn show(&self, visible: bool) -> Result<()> {
        self.visual.SetIsVisible(visible && self.positioned)
    }
}

impl Backdrop {
    fn new(state: &State, material: SearchMaterial) -> Result<Self> {
        let composition = Composition::new(state, state.window.get(), material)?;
        let caret = Caret::new(&composition.root)?;
        let text_brush = unsafe {
            composition
                .canvas
                .CreateSolidColorBrush(&color(0, 1.0), None)?
        };
        Ok(Self {
            selected: material,
            composition,
            text_brush,
            caret,
            edit: None,
            search_text: None,
            search_text_active: None,
            #[cfg(feature = "text-render-lab")]
            direct_edit: super::text_render_lab::DirectEdit::new(state)?,
        })
    }

    fn paint(&mut self, state: &State, width: u32, height: u32) -> Result<()> {
        let (tint, radius) = {
            let app = state.app.borrow();
            let prefs = &app.settings().preferences;
            let tint = if self.selected == SearchMaterial::Solid {
                100
            } else {
                prefs.search_tint_percent()
            };
            (tint, prefs.search_corner_radius)
        };
        let colors = state.search_colors();
        let mut caret = None;
        self.composition.paint(
            (width, height),
            state.dpi.get(),
            radius,
            state.search_is_dark(),
            |painter, canvas| {
                unsafe { canvas.Clear(Some(&color(colors.surface, tint as f32 / 100.0))) };
                colors.paint_border(
                    painter,
                    (width as i32, height as i32),
                    state.dpi.get(),
                    state.search_control_focused(),
                    radius,
                );
                #[cfg(feature = "text-render-lab")]
                let edit = if state.text_sample.get().is_some() {
                    self.direct_edit
                        .as_ref()
                        .map(|direct| direct.draw(state, canvas, &self.text_brush).map(|()| None))
                        .unwrap_or_else(|| EditLayer::draw(&mut self.edit, state, canvas))
                } else {
                    Self::draw_search_text(
                        &mut self.search_text,
                        &mut self.search_text_active,
                        &mut self.edit,
                        state,
                        canvas,
                        &self.text_brush,
                    )
                };
                #[cfg(not(feature = "text-render-lab"))]
                let edit = Self::draw_search_text(
                    &mut self.search_text,
                    &mut self.search_text_active,
                    &mut self.edit,
                    state,
                    canvas,
                    &self.text_brush,
                );
                state.paint_composed_engine(painter);
                painter.text(
                    state.hint.get(),
                    state.search_hint_area(width as i32),
                    if state.browser_failure.get().is_some() {
                        colors.error
                    } else {
                        colors.muted
                    },
                    Label::Hint,
                );
                caret = edit?;
                Ok(())
            },
        )?;
        self.caret
            .update(caret, state.dpi.get(), colors.text, state.caret_on.get())?;
        #[cfg(feature = "text-render-lab")]
        super::text_render_lab::record_draw(
            state,
            self.direct_edit
                .as_ref()
                .map_or(super::text_render_lab::Mode::Current, |edit| edit.mode()),
        );
        Ok(())
    }

    fn draw_search_text(
        search_text: &mut Option<SearchText>,
        active: &mut Option<bool>,
        edit: &mut Option<EditLayer>,
        state: &State,
        canvas: &ID2D1DeviceContext,
        brush: &ID2D1SolidColorBrush,
    ) -> Result<Option<D2D_RECT_F>> {
        let result = (|| {
            if search_text.is_none() {
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
        if active.replace(false) != Some(false) {
            state.diagnostics.sample("search_text_fallback");
            state.edit_dirty.set(true);
        }
        EditLayer::draw(edit, state, canvas)
    }
}

pub(super) fn set_material(hwnd: RawWindow, selected: SearchMaterial, dark: bool) -> bool {
    set_material_frame(hwnd, selected, dark, selected != SearchMaterial::Solid)
}

pub(super) fn set_material_frame(
    hwnd: RawWindow,
    selected: SearchMaterial,
    dark: bool,
    glass: bool,
) -> bool {
    let material = DWMSBT_NONE;
    let host = i32::from(selected == SearchMaterial::Acrylic);
    let dark = i32::from(dark);
    let margin = if glass { -1 } else { 0 };
    let margins = MARGINS {
        cxLeftWidth: margin,
        cxRightWidth: margin,
        cyTopHeight: margin,
        cyBottomHeight: margin,
    };
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&dark as *const i32).cast(),
            4,
        );
        let host_backdrop = DwmSetWindowAttribute(
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
        backdrop >= 0 && frame >= 0 && (host == 0 || host_backdrop >= 0)
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
    pub(super) fn show_composed_caret(&self, visible: bool) {
        if let Ok(backdrop) = self.backdrop.try_borrow()
            && let Some(backdrop) = backdrop.as_ref()
        {
            let _ = backdrop.caret.show(visible);
        }
    }

    pub(super) fn refresh_search_appearance(&self) {
        let window = self.window.get();
        if window.is_null() {
            return;
        }
        self.edit_dirty.set(true);
        self.search_text_dirty.set(true);
        unsafe {
            use windows_sys::Win32::{Graphics::Gdi::*, UI::WindowsAndMessaging::IsWindowVisible};
            let visible = IsWindowVisible(window) != 0;
            if visible || self.composing_controls.get() {
                self.prepare_backdrop();
            }
            if visible {
                RedrawWindow(
                    window,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    RDW_INVALIDATE | RDW_ALLCHILDREN,
                );
            }
        }
    }

    pub(super) fn prepare_backdrop(&self) {
        let hwnd = self.window.get();
        #[cfg(feature = "text-render-lab")]
        let force_solid = super::text_render_lab::solid_window();
        #[cfg(not(feature = "text-render-lab"))]
        let force_solid = false;
        let (material, dark, radius) = {
            let app = self.app.borrow();
            let prefs = &app.settings().preferences;
            (
                prefs.search_material,
                prefs.search_tone.is_dark(self.system_dark.get()),
                prefs.search_corner_radius,
            )
        };
        let selected =
            if force_solid || high_contrast() || !super::screen_shade::transparency_enabled() {
                SearchMaterial::Solid
            } else {
                material
            };
        let previous = self.backdrop_material.get();
        let compose = !force_solid && (selected != SearchMaterial::Solid || radius > 0);
        let next = if compose && set_material_frame(hwnd, selected, dark, true) {
            Some(selected)
        } else {
            set_material(hwnd, SearchMaterial::Solid, dark);
            None
        };
        self.backdrop_material.set(next);
        if previous != next {
            self.release_backdrop();
        }
        if previous.is_some() != next.is_some() {
            self.update_search_corners();
        }
        if next.is_none() {
            self.diagnostics.sample("backdrop_solid");
        }
    }

    pub(super) fn paint_backdrop(&self, bounds: &RECT) -> bool {
        if bounds.right <= 0 || bounds.bottom <= 0 {
            return false;
        }
        let Some(material) = self.backdrop_material.get() else {
            return false;
        };
        let Ok(mut backdrop) = self.backdrop.try_borrow_mut() else {
            return false;
        };
        let newly_created = backdrop.is_none();
        if backdrop.is_none() {
            self.diagnostics.sample("backdrop_create_begin");
            *backdrop = Backdrop::new(self, material).ok();
            self.diagnostics.sample("backdrop_create_done");
        }
        if backdrop.as_mut().is_some_and(|b| {
            b.paint(self, bounds.right as u32, bounds.bottom as u32)
                .is_ok()
        }) {
            self.use_composed_controls(true);
            if newly_created {
                self.diagnostics.sample(match material {
                    SearchMaterial::Mica => "backdrop_mica",
                    SearchMaterial::Acrylic => "backdrop_acrylic",
                    SearchMaterial::Solid => "backdrop_solid_composed",
                });
            }
            return true;
        }
        *backdrop = None;
        drop(backdrop);
        self.backdrop_material.set(None);
        self.use_composed_controls(false);
        set_material(
            self.window.get(),
            SearchMaterial::Solid,
            self.search_is_dark(),
        );
        self.update_search_corners();
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
            backdrop.composition.painter.reset_images();
        }
    }
}
