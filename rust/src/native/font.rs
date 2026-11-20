use std::{os::windows::ffi::OsStrExt, path::Path, ptr::null};
use windows::{
    Win32::{
        Foundation::E_FAIL,
        Graphics::DirectWrite::{
            DWRITE_FACTORY_TYPE_ISOLATED, DWRITE_FONT_SIMULATIONS_NONE, DWriteCreateFactory,
            IDWriteFactory3, IDWriteFontCollection,
        },
    },
    core::{Error, Interface, PCWSTR, Result},
};
use windows_sys::Win32::Graphics::Gdi::{AddFontResourceExW, FR_PRIVATE, RemoveFontResourceExW};

pub(super) struct FileFont {
    path: Vec<u16>,
    pub family: String,
    pub collection: IDWriteFontCollection,
}

impl FileFont {
    pub(super) fn load(path: &Path) -> Result<Self> {
        let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        if path[..path.len() - 1].contains(&0) {
            return Err(Error::from_hresult(E_FAIL));
        }
        unsafe {
            let factory: IDWriteFactory3 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED)?;
            let reference = factory.CreateFontFaceReference2(
                PCWSTR(path.as_ptr()),
                None,
                0,
                DWRITE_FONT_SIMULATIONS_NONE,
            )?;
            let builder = factory.CreateFontSetBuilder()?;
            builder.AddFontFaceReference2(&reference)?;
            let collection = factory.CreateFontCollectionFromFontSet(&builder.CreateFontSet()?)?;
            let names = collection.GetFontFamily(0)?.GetFamilyNames()?;
            let mut name = vec![0; names.GetStringLength(0)? as usize + 1];
            names.GetString(0, &mut name)?;
            let family = String::from_utf16_lossy(&name[..name.len() - 1]);
            let collection = collection.cast()?;
            if AddFontResourceExW(path.as_ptr(), FR_PRIVATE, null()) == 0 {
                return Err(Error::from_hresult(E_FAIL));
            }
            Ok(Self {
                path,
                family,
                collection,
            })
        }
    }
}

impl Drop for FileFont {
    fn drop(&mut self) {
        unsafe { RemoveFontResourceExW(self.path.as_ptr(), FR_PRIVATE, null()) };
    }
}

impl super::State {
    pub(super) fn drawing_font(&self) -> (String, Option<IDWriteFontCollection>) {
        let family = self.font_family();
        let collection = self
            .file_font
            .borrow()
            .as_ref()
            .map(|font| font.collection.clone());
        (family, collection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_private_font_from_file_and_rejects_missing_file() {
        let directory = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap()).join("Fonts");
        let font = FileFont::load(&directory.join("arial.ttf")).unwrap();
        assert_eq!(font.family, "Arial");
        assert_eq!(unsafe { font.collection.GetFontFamilyCount() }, 1);
        assert!(FileFont::load(&directory.join("yisou-missing.ttf")).is_err());
    }
}
