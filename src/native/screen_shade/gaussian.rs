use windows::{
    Foundation::{IPropertyValue, PropertyValue},
    Graphics::Effects::{
        IGraphicsEffect, IGraphicsEffect_Impl, IGraphicsEffectSource, IGraphicsEffectSource_Impl,
    },
    UI::Composition::{
        CompositionEffectBrush, CompositionEffectSourceParameter, Compositor,
        Desktop::DesktopWindowTarget,
    },
    Win32::{
        Foundation::{E_INVALIDARG, E_NOTIMPL, HWND},
        Graphics::Direct2D::{
            CLSID_D2D1GaussianBlur, Common::D2D1_BORDER_MODE_HARD,
            D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED,
        },
        System::WinRT::{
            Composition::ICompositorDesktopInterop,
            Graphics::Direct2D::{
                GRAPHICS_EFFECT_PROPERTY_MAPPING, IGraphicsEffectD2D1Interop,
                IGraphicsEffectD2D1Interop_Impl,
            },
        },
    },
    core::{Error, GUID, HSTRING, Interface, PCWSTR, Result, implement},
};
use windows_numerics::Vector2;

pub(super) struct Backdrop {
    compositor: Compositor,
    brush: CompositionEffectBrush,
    targets: Vec<DesktopWindowTarget>,
}

impl Backdrop {
    pub(super) fn new(amount: f32) -> Result<Self> {
        crate::native::composition_queue::prepare()?;
        let compositor = Compositor::new()?;
        let input = HSTRING::from("Desktop");
        let effect: IGraphicsEffect = Gaussian {
            source: CompositionEffectSourceParameter::Create(&input)?.cast()?,
            amount,
        }
        .into();
        let brush = compositor.CreateEffectFactory(&effect)?.CreateBrush()?;
        brush.SetSourceParameter(&input, &compositor.CreateBackdropBrush()?)?;
        Ok(Self {
            compositor,
            brush,
            targets: Vec::new(),
        })
    }

    pub(super) fn attach(&mut self, window: windows_sys::Win32::Foundation::HWND) -> Result<()> {
        let interop = self.compositor.cast::<ICompositorDesktopInterop>()?;
        let target = unsafe { interop.CreateDesktopWindowTarget(HWND(window), true)? };
        let visual = self.compositor.CreateSpriteVisual()?;
        visual.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        visual.SetBrush(&self.brush)?;
        target.SetRoot(&visual)?;
        self.targets.push(target);
        Ok(())
    }
}

// Describe the system Direct2D effect without a runtime Win2D dependency.
#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct Gaussian {
    source: IGraphicsEffectSource,
    amount: f32,
}

impl IGraphicsEffectSource_Impl for Gaussian_Impl {}

impl IGraphicsEffect_Impl for Gaussian_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(HSTRING::from("Gaussian"))
    }

    fn SetName(&self, _name: &HSTRING) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
}

impl IGraphicsEffectD2D1Interop_Impl for Gaussian_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1GaussianBlur)
    }

    fn GetNamedPropertyMapping(
        &self,
        _name: &PCWSTR,
        _index: *mut u32,
        _mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        match index {
            0 => PropertyValue::CreateSingle(self.amount),
            1 => PropertyValue::CreateUInt32(D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED.0 as u32),
            2 => PropertyValue::CreateUInt32(D2D1_BORDER_MODE_HARD.0 as u32),
            _ => return Err(Error::from_hresult(E_INVALIDARG)),
        }?
        .cast()
    }

    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }

    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(Error::from_hresult(E_INVALIDARG))
        }
    }
}
