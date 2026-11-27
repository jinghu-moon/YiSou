use windows::{
    Graphics::{IGeometrySource2D, IGeometrySource2D_Impl},
    UI::Composition::CompositionPath,
    Win32::{
        Graphics::Direct2D::{Common::*, D2D1_ROUNDED_RECT, ID2D1Factory, ID2D1Geometry},
        System::WinRT::Graphics::Direct2D::{
            IGeometrySource2DInterop, IGeometrySource2DInterop_Impl,
        },
    },
    core::{Interface, Ref, Result, implement},
};
use windows_numerics::Vector2;
use windows_sys::Win32::{
    Foundation::RECT,
    Graphics::Gdi::{ExtCreateRegion, HRGN, RDH_RECTANGLES, RGNDATAHEADER},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CornerShape {
    radius: f32,
    exponent: f32,
}

impl CornerShape {
    pub(super) fn new(requested: f32, width: f32, height: f32) -> Self {
        let limit = width.min(height).max(0.0) / 2.0;
        let radius = requested.clamp(0.0, limit);
        // Retain the quartic curve for typical corners and ease into a circle at full radius.
        let transition = limit * (2.0 / 3.0);
        let exponent = if radius <= transition {
            4.0
        } else {
            let t = (radius - transition) / (limit - transition);
            4.0 - 2.0 * t * t * (3.0 - 2.0 * t)
        };
        Self { radius, exponent }
    }

    pub(super) fn inset(self, amount: f32) -> Self {
        Self {
            radius: (self.radius - amount).max(0.0),
            ..self
        }
    }

    fn companion(self, t: f32) -> f32 {
        match self.exponent {
            2.0 => (1.0 - t * t).max(0.0).sqrt(),
            4.0 => (1.0 - t.powi(4)).max(0.0).sqrt().sqrt(),
            exponent => (1.0 - t.powf(exponent)).max(0.0).powf(exponent.recip()),
        }
    }
}

pub(super) fn geometry(
    factory: &ID2D1Factory,
    area: D2D_RECT_F,
    corner: CornerShape,
) -> Result<ID2D1Geometry> {
    const STEPS: usize = 32;
    let radius = corner.radius;
    unsafe {
        if radius == 0.0 {
            return factory.CreateRectangleGeometry(&area)?.cast();
        }
        if corner.exponent == 2.0 {
            return factory
                .CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT {
                    rect: area,
                    radiusX: radius,
                    radiusY: radius,
                })?
                .cast();
        }
        let path = factory.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(
            Vector2 {
                X: area.left + radius,
                Y: area.top,
            },
            D2D1_FIGURE_BEGIN_FILLED,
        );
        let centers = [
            (area.right - radius, area.top + radius),
            (area.right - radius, area.bottom - radius),
            (area.left + radius, area.bottom - radius),
            (area.left + radius, area.top + radius),
        ];
        // Sample to the diagonal and mirror to avoid the steep end of the curve.
        let diagonal = 0.5_f32.powf(corner.exponent.recip());
        let samples: [Vector2; 2 * STEPS + 1] = std::array::from_fn(|i| {
            let t = diagonal * i.min(2 * STEPS - i) as f32 / STEPS as f32;
            if i <= STEPS {
                Vector2 {
                    X: t,
                    Y: -corner.companion(t),
                }
            } else {
                Vector2 {
                    X: corner.companion(t),
                    Y: -t,
                }
            }
        });
        for (corner, (cx, cy)) in centers.into_iter().enumerate() {
            let points = samples.map(|Vector2 { X: x, Y: y }| {
                let (x, y) = match corner {
                    0 => (x, y),
                    1 => (-y, x),
                    2 => (-x, -y),
                    _ => (y, -x),
                };
                Vector2 {
                    X: cx + radius * x,
                    Y: cy + radius * y,
                }
            });
            sink.AddLines(&points);
        }
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        path.cast()
    }
}

#[implement(IGeometrySource2D, IGeometrySource2DInterop)]
struct GeometrySource {
    factory: ID2D1Factory,
    area: D2D_RECT_F,
    corner: CornerShape,
}

impl IGeometrySource2D_Impl for GeometrySource_Impl {}

impl IGeometrySource2DInterop_Impl for GeometrySource_Impl {
    fn GetGeometry(&self) -> Result<ID2D1Geometry> {
        geometry(&self.factory, self.area, self.corner)
    }

    fn TryGetGeometryUsingFactory(&self, factory: Ref<ID2D1Factory>) -> Result<ID2D1Geometry> {
        geometry(factory.ok()?, self.area, self.corner)
    }
}

pub(super) fn composition_path(
    factory: ID2D1Factory,
    area: D2D_RECT_F,
    corner: CornerShape,
) -> Result<CompositionPath> {
    let source: IGeometrySource2D = GeometrySource {
        factory,
        area,
        corner,
    }
    .into();
    CompositionPath::Create(&source)
}

pub(super) fn window_region(width: i32, height: i32, requested: f32, antialiased: bool) -> HRGN {
    let corner = CornerShape::new(requested, width as f32, height as f32);
    let radius = corner.radius;
    if radius == 0.0 {
        return std::ptr::null_mut();
    }
    let rows = (radius.ceil() as i32).min(height / 2);
    let mut bands = Vec::<RECT>::with_capacity((2 * rows + 1) as usize);
    let mut add = |top, bottom, cut| {
        let right = width - cut;
        if let Some(last) = bands.last_mut()
            && last.left == cut
        {
            last.bottom = bottom;
        } else {
            bands.push(RECT {
                left: cut,
                top,
                right,
                bottom,
            });
        }
    };
    let cut = |y: i32| {
        // Allow the compositor to preserve partial coverage at the edge.
        let sample = if antialiased { 1.0 } else { 0.5 };
        let t = (1.0 - (y as f32 + sample) / radius).max(0.0);
        let inset = radius * (1.0 - corner.companion(t));
        if antialiased {
            inset.floor() as i32
        } else {
            (inset - 0.5).ceil() as i32
        }
    };
    for y in 0..rows {
        add(y, y + 1, cut(y));
    }
    if rows < height - rows {
        add(rows, height - rows, 0);
    }
    for y in (0..rows).rev() {
        add(height - y - 1, height - y, cut(y));
    }
    let header_size = size_of::<RGNDATAHEADER>();
    let band_size = size_of_val(bands.as_slice());
    let byte_count = header_size + band_size;
    let header = RGNDATAHEADER {
        dwSize: header_size as u32,
        iType: RDH_RECTANGLES,
        nCount: bands.len() as u32,
        nRgnSize: band_size as u32,
        rcBound: RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        },
    };
    let mut data = vec![header; byte_count.div_ceil(header_size)];
    unsafe {
        std::ptr::copy_nonoverlapping(
            bands.as_ptr().cast::<u8>(),
            data.as_mut_ptr().cast::<u8>().add(header_size),
            band_size,
        );
        ExtCreateRegion(std::ptr::null(), byte_count as u32, data.as_ptr().cast())
    }
}
