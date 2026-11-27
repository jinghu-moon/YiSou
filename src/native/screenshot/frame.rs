use super::{capture::same_rect, chrome::Chrome, perf};
use windows::{
    UI::{
        Color,
        Composition::{
            CompositionColorBrush, CompositionEllipseGeometry, Compositor,
            Desktop::DesktopWindowTarget, ShapeVisual, SpriteVisual, VisualCollection,
        },
    },
    Win32::{
        Foundation::{E_FAIL, HWND},
        System::WinRT::Composition::ICompositorDesktopInterop,
    },
    core::{Error, Interface, Result},
};
use windows_numerics::{Vector2, Vector3};
use windows_sys::Win32::Foundation::{HWND as RawWindow, RECT};

const BLUE: Color = Color {
    A: 255,
    R: 31,
    G: 117,
    B: 219,
};
const WHITE: Color = Color {
    A: 255,
    R: 255,
    G: 255,
    B: 255,
};
const SHADE: Color = Color {
    A: 115,
    R: 0,
    G: 0,
    B: 0,
};

pub(super) struct Frame {
    _target: DesktopWindowTarget,
    shade: [SpriteVisual; 4],
    border: [SpriteVisual; 4],
    grips: [ShapeVisual; 8],
    circle: CompositionEllipseGeometry,
    layout: Option<(RECT, i32, i32)>,
}

impl Frame {
    pub fn new(hwnd: RawWindow) -> Result<Self> {
        crate::native::composition_queue::prepare()?;
        let compositor = Compositor::new()?;
        let interop = compositor.cast::<ICompositorDesktopInterop>()?;
        let target = unsafe { interop.CreateDesktopWindowTarget(HWND(hwnd), true)? };
        let root = compositor.CreateContainerVisual()?;
        let children = root.Children()?;
        let shade = compositor.CreateColorBrushWithColor(SHADE)?;
        let blue = compositor.CreateColorBrushWithColor(BLUE)?;
        let white = compositor.CreateColorBrushWithColor(WHITE)?;
        let shade = rectangles(&compositor, &children, &shade)?;
        let border = rectangles(&compositor, &children, &blue)?;
        let circle = compositor.CreateEllipseGeometry()?;
        let mut grips = Vec::with_capacity(8);
        for _ in 0..8 {
            let grip = compositor.CreateShapeVisual()?;
            let shape = compositor.CreateSpriteShapeWithGeometry(&circle)?;
            shape.SetFillBrush(&white)?;
            shape.SetStrokeBrush(&blue)?;
            shape.SetStrokeThickness(1.5)?;
            grip.Shapes()?.Append(&shape)?;
            grip.SetIsVisible(false)?;
            children.InsertAtTop(&grip)?;
            grips.push(grip);
        }
        target.SetRoot(&root)?;
        Ok(Self {
            _target: target,
            shade,
            border,
            grips: grips.try_into().map_err(|_| Error::from_hresult(E_FAIL))?,
            circle,
            layout: None,
        })
    }

    pub fn update(
        &mut self,
        desktop: RECT,
        area: Option<RECT>,
        chrome: &Chrome,
        show_grips: bool,
    ) -> Result<()> {
        let stroke = chrome.frame_width();
        let radius = if show_grips { chrome.px(6) } else { 0 };
        let area = area.unwrap_or_default();
        if self.layout.is_some_and(|(old, width, grip)| {
            same_rect(old, area) && width == stroke && grip == radius
        }) {
            return Ok(());
        }
        let _span = perf::span("compose_frame");
        let width = desktop.right - desktop.left;
        let height = desktop.bottom - desktop.top;
        let band = stroke
            .min(area.right - area.left)
            .min(area.bottom - area.top);
        let inner_top = (area.top + band).min(area.bottom);
        let inner_bottom = (area.bottom - band).max(inner_top);
        for (visual, bounds) in self.shade.iter().zip([
            (0, 0, width, area.top),
            (0, area.bottom, width, height),
            (0, area.top, area.left, area.bottom),
            (area.right, area.top, width, area.bottom),
        ]) {
            place(visual, bounds)?;
        }
        for (visual, bounds) in self.border.iter().zip([
            (area.left, area.top, area.right, inner_top),
            (area.left, inner_bottom, area.right, area.bottom),
            (area.left, inner_top, area.left + band, inner_bottom),
            (area.right - band, inner_top, area.right, inner_bottom),
        ]) {
            place(visual, bounds)?;
        }
        if radius > 0 && self.layout.is_none_or(|(_, _, old)| old != radius) {
            let diameter = 2 * radius;
            self.circle.SetCenter(size(radius, radius))?;
            self.circle.SetRadius(size(radius - 1, radius - 1))?;
            for grip in &self.grips {
                grip.SetSize(size(diameter, diameter))?;
            }
        }
        for (grip, center) in self.grips.iter().zip(chrome.grip_centers(area)) {
            let center = center.filter(|_| radius > 0);
            if let Some(point) = center {
                grip.SetOffset(offset(point.x - radius, point.y - radius))?;
            }
            grip.SetIsVisible(center.is_some())?;
        }
        // One visual tree changes together when the UI thread returns to its message loop.
        self.layout = Some((area, stroke, radius));
        Ok(())
    }
}

fn rectangles(
    compositor: &Compositor,
    children: &VisualCollection,
    brush: &CompositionColorBrush,
) -> Result<[SpriteVisual; 4]> {
    let mut visuals = Vec::with_capacity(4);
    for _ in 0..4 {
        let visual = compositor.CreateSpriteVisual()?;
        visual.SetBrush(brush)?;
        children.InsertAtTop(&visual)?;
        visuals.push(visual);
    }
    visuals.try_into().map_err(|_| Error::from_hresult(E_FAIL))
}

fn place(visual: &SpriteVisual, (left, top, right, bottom): (i32, i32, i32, i32)) -> Result<()> {
    visual.SetOffset(offset(left, top))?;
    visual.SetSize(size(right - left, bottom - top))
}

fn offset(x: i32, y: i32) -> Vector3 {
    Vector3 {
        X: x as f32,
        Y: y as f32,
        Z: 0.0,
    }
}

fn size(x: i32, y: i32) -> Vector2 {
    Vector2 {
        X: x as f32,
        Y: y as f32,
    }
}
