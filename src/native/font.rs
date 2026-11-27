use std::{os::windows::ffi::OsStrExt, path::Path, ptr::null};
use windows::{
    Win32::{
        Foundation::E_FAIL,
        Graphics::DirectWrite::{
            DWRITE_FACTORY_TYPE_ISOLATED, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_SIMULATIONS_NONE,
            DWriteCreateFactory, IDWriteFactory, IDWriteFactory3, IDWriteFontCollection,
            IDWriteLocalizedStrings,
        },
    },
    core::{Error, Interface, PCWSTR, Result},
};
use windows_sys::Win32::Graphics::Gdi::*;
use yisou::settings::FontFace;

fn localized_name(names: &IDWriteLocalizedStrings) -> Result<String> {
    let mut locale = [0; 85];
    let mut index = 0;
    let mut exists = false.into();
    unsafe {
        if windows_sys::Win32::Globalization::GetUserDefaultLocaleName(locale.as_mut_ptr(), 85) > 0
        {
            names.FindLocaleName(PCWSTR(locale.as_ptr()), &mut index, &mut exists)?;
        }
        if !exists.as_bool() {
            names.FindLocaleName(windows::core::w!("en-US"), &mut index, &mut exists)?;
        }
        if !exists.as_bool() {
            index = 0;
        }
    }
    name_at(names, index)
}

fn name_at(names: &IDWriteLocalizedStrings, index: u32) -> Result<String> {
    let mut text = vec![0; unsafe { names.GetStringLength(index)? } as usize + 1];
    unsafe { names.GetString(index, &mut text)? };
    text.pop();
    Ok(String::from_utf16_lossy(&text))
}

fn face_name(names: &IDWriteLocalizedStrings, english: bool) -> Option<String> {
    let locales = if english {
        [
            windows::core::w!("en-US"),
            windows::core::w!("en"),
            windows::core::w!("en-GB"),
            windows::core::w!("en-CA"),
        ]
    } else {
        [
            windows::core::w!("zh-CN"),
            windows::core::w!("zh-Hans"),
            windows::core::w!("zh"),
            windows::core::w!("zh-SG"),
        ]
    };
    for locale in locales {
        let mut index = 0;
        let mut exists = false.into();
        if unsafe { names.FindLocaleName(locale, &mut index, &mut exists) }.is_ok()
            && exists.as_bool()
            && let Ok(name) = name_at(names, index)
            && !name.trim().is_empty()
            && (english
                || name
                    .chars()
                    .any(|character| ('\u{3400}'..='\u{9fff}').contains(&character)))
        {
            return Some(name);
        }
    }
    None
}

fn standard_face_name(face: FontFace, english: bool) -> String {
    let label = |zh, en| if english { en } else { zh };
    let stretch = match face.stretch {
        1 => label("极窄", "Ultra Condensed"),
        2 => label("特窄", "Extra Condensed"),
        3 => label("窄体", "Condensed"),
        4 => label("半窄", "Semi Condensed"),
        6 => label("半宽", "Semi Expanded"),
        7 => label("宽体", "Expanded"),
        8 => label("特宽", "Extra Expanded"),
        9 => label("极宽", "Ultra Expanded"),
        _ => "",
    };
    let style = match face.style {
        1 => label("倾斜体", "Oblique"),
        2 => label("斜体", "Italic"),
        _ => "",
    };
    let custom_weight;
    let weight = match face.weight {
        100 => label("极细体", "Thin"),
        200 => label("特细体", "Extra Light"),
        300 => label("细体", "Light"),
        350 => label("次细体", "Semi Light"),
        400 if stretch.is_empty() && style.is_empty() => label("常规", "Regular"),
        400 => "",
        500 => label("中等", "Medium"),
        600 => label("半粗体", "Semibold"),
        700 => label("粗体", "Bold"),
        800 => label("特粗体", "Extra Bold"),
        900 => label("极粗体", "Black"),
        950 => label("超粗体", "Extra Black"),
        weight => {
            custom_weight = format!("{} {weight}", label("字重", "Weight"));
            &custom_weight
        }
    };
    [stretch, weight, style]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn system_fonts() -> Result<IDWriteFontCollection> {
    let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
    let mut collection = None;
    unsafe { factory.GetSystemFontCollection(&mut collection, false)? };
    collection.ok_or(Error::from_hresult(E_FAIL))
}

pub(super) fn families(collection: &IDWriteFontCollection) -> Vec<String> {
    let mut families: Vec<_> = unsafe { 0..collection.GetFontFamilyCount() }
        .filter_map(|index| unsafe {
            localized_name(
                &collection
                    .GetFontFamily(index)
                    .ok()?
                    .GetFamilyNames()
                    .ok()?,
            )
            .ok()
        })
        .collect();
    families.sort_by_cached_key(|name| name.to_lowercase());
    families.dedup();
    families
}

pub(super) fn faces(
    collection: &IDWriteFontCollection,
    family: &str,
    english: bool,
) -> Vec<(String, FontFace)> {
    let mut index = 0;
    let mut exists = false.into();
    let name = super::wide(family);
    unsafe {
        if collection
            .FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists)
            .is_err()
            || !exists.as_bool()
        {
            return Vec::new();
        }
        let Ok(family) = collection.GetFontFamily(index) else {
            return Vec::new();
        };
        (0..family.GetFontCount())
            .filter_map(|index| {
                let font = family.GetFont(index).ok()?;
                let face = FontFace {
                    weight: font.GetWeight().0 as u16,
                    stretch: font.GetStretch().0 as u8,
                    style: font.GetStyle().0 as u8,
                };
                let name = font
                    .GetFaceNames()
                    .ok()
                    .and_then(|names| face_name(&names, english))
                    .unwrap_or_else(|| standard_face_name(face, english));
                Some((name, face))
            })
            .collect()
    }
}

pub(super) fn preview_font(family: &str, face: FontFace, size: i32, dpi: u32) -> HFONT {
    unsafe {
        CreateFontW(
            -super::dip(size, dpi),
            0,
            0,
            0,
            i32::from(face.weight),
            u32::from(face.style != 0),
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            super::wide(family).as_ptr(),
        )
    }
}

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
    pub(super) fn font_face(&self) -> FontFace {
        #[cfg(feature = "text-render-lab")]
        if self.text_sample.get().is_some() {
            return FontFace::default();
        }
        self.app.borrow().settings().preferences.search_font_face
    }

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
