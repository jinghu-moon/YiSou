use super::{
    components::{CONTROL_HEIGHT, NumberStepper, Page, WIDTH},
    *,
};
use crate::native::paint::{Label, Painter};
use std::collections::HashMap;
use windows::Win32::Graphics::{
    Direct2D::Common::D2D_RECT_F,
    DirectWrite::{IDWriteFontCollection, IDWriteTextFormat},
};
use yisou::settings::{FontFace, FontPreviewScript};

pub(super) const FAMILY: usize = 181;
pub(super) const FACE: usize = 182;
pub(super) const SCRIPT: usize = 183;
pub(super) const SIZE: usize = 184;
pub(super) const PREVIEW: usize = 185;
pub(super) const FILTER: usize = 186;
pub(super) const CANCEL: usize = 187;
pub(super) const APPLY: usize = 188;
pub(super) const MORE: usize = 189;
pub(super) const DECREASE: usize = 193;
pub(super) const INCREASE: usize = 194;
pub(super) const FILTER_TIMER: usize = 19;

const FONT_STEPPER: NumberStepper = NumberStepper {
    input: SIZE,
    decrease: DECREASE,
    increase: INCREASE,
    range: yisou::settings::MIN_SEARCH_FONT_SIZE..=yisou::settings::MAX_SEARCH_FONT_SIZE,
    unit: "DIP",
};

const SCRIPTS: [(FontPreviewScript, &str, &str); 5] = [
    (
        FontPreviewScript::SimplifiedChinese,
        "简体中文",
        "复杂中文 · 搜索网页 123",
    ),
    (
        FontPreviewScript::TraditionalChinese,
        "繁體中文",
        "鬱鑿齒 · 搜尋網頁 123",
    ),
    (
        FontPreviewScript::Latin,
        "拉丁文",
        "The quick brown fox · Search 123",
    ),
    (
        FontPreviewScript::Japanese,
        "日语",
        "日本語の文字を検索 123",
    ),
    (FontPreviewScript::Korean, "韩语", "한글로 웹 검색 123"),
];

pub(super) struct Picker {
    collection: IDWriteFontCollection,
    family_formats: HashMap<usize, Option<IDWriteTextFormat>>,
    families: Vec<String>,
    visible: Vec<usize>,
    faces: Vec<(String, FontFace)>,
    family: usize,
    face: usize,
    script: usize,
    size: i32,
    pub previous_scroll: i32,
}

impl Picker {
    pub fn new(state: &State, previous_scroll: i32) -> Result<Self, String> {
        let collection = super::super::font::system_fonts().map_err(|error| error.to_string())?;
        let families = super::super::font::families(&collection);
        if families.is_empty() {
            return Err("没有可用的系统字体".into());
        }
        let current = state.font_family();
        let family = families
            .iter()
            .position(|name| *name == current)
            .unwrap_or(0);
        let app = state.app.borrow();
        let prefs = &app.settings().preferences;
        let script = SCRIPTS
            .iter()
            .position(|s| s.0 == prefs.font_preview_script)
            .unwrap_or(0);
        let mut picker = Self {
            collection,
            family_formats: HashMap::new(),
            visible: (0..families.len()).collect(),
            families,
            faces: Vec::new(),
            family,
            face: 0,
            script,
            size: prefs.search_font_size,
            previous_scroll,
        };
        picker.select_family(
            family,
            prefs.search_font_face,
            english_selected(prefs.language),
        );
        Ok(picker)
    }

    fn select_family(&mut self, family: usize, preferred: FontFace, english: bool) {
        self.family = family;
        self.faces = super::super::font::faces(&self.collection, &self.families[family], english);
        self.face = self
            .faces
            .iter()
            .enumerate()
            .min_by_key(|(_, (_, face))| {
                u32::from(face.weight.abs_diff(preferred.weight))
                    + u32::from(face.stretch.abs_diff(preferred.stretch)) * 1000
                    + u32::from(face.style != preferred.style) * 10000
            })
            .map_or(0, |(index, _)| index);
    }

    fn face(&self) -> FontFace {
        self.faces
            .get(self.face)
            .map_or_else(FontFace::default, |(_, face)| *face)
    }
}

pub(super) fn paint_family(p: &mut Painter, option: usize, name: &str, area: D2D_RECT_F, rgb: u32) {
    let format = PANEL.with_borrow_mut(|panel| {
        let picker = panel.font_picker.as_mut()?;
        let family = *picker.visible.get(option)?;
        picker
            .family_formats
            .entry(family)
            .or_insert_with(|| {
                p.family_text_format(&picker.families[family], &picker.collection)
                    .ok()
            })
            .clone()
    });
    if let Some(format) = format {
        p.text_with_format(name, area, rgb, &format);
    } else {
        p.text(name, area, rgb, Label::Body);
    }
}

pub(super) fn display(hwnd: HWND) {
    let page = Page(hwnd);
    page.heading("选择系统字体");
    page.section("字体", 98, 285, &[]);
    let search = page.field(FILTER, "筛选字体", "", 14, 105, 508, STRETCH);
    cue(search, "搜索字体名称");
    page.body("字体家族", place(14, 170, 290, 22, STRETCH));
    page.body("字形", place(320, 170, 202, 22, RIGHT));
    dropdown::create(
        hwnd,
        FAMILY,
        std::iter::empty::<&str>(),
        0,
        place(14, 193, 289, CONTROL_HEIGHT, STRETCH),
    );
    dropdown::create(
        hwnd,
        FACE,
        std::iter::empty::<&str>(),
        0,
        place(320, 193, 202, CONTROL_HEIGHT, RIGHT),
    );
    dropdown::create(
        hwnd,
        SCRIPT,
        SCRIPTS.iter().map(|s| tr(s.1)),
        0,
        place(14, 243, 164, CONTROL_HEIGHT, 0),
    );
    let (script, size) = PANEL.with_borrow(|p| {
        let picker = p.font_picker.as_ref().unwrap();
        (picker.script, picker.size)
    });
    unsafe { SendMessageW(GetDlgItem(hwnd, SCRIPT as i32), CB_SETCURSEL, script, 0) };
    page.body("字号", place(196, 245, 55, 26, 0));
    FONT_STEPPER.create(hwnd, size, 255, 243, 0, ["减小字号", "增大字号"]);
    page.section("预览", 329, 451, &[]);
    let preview = caption(
        hwnd,
        "",
        place(14, 332, 508, 116, STRETCH),
        fluent::FontRole::Preview,
    );
    unsafe { SetWindowLongPtrW(preview, GWLP_ID, PREVIEW as isize) };
    page.buttons(0, 467, 0, &[(MORE, "显示更多字体")]);
    page.buttons(WIDTH, 467, RIGHT, &[(CANCEL, "取消"), (APPLY, "应用")]);
    page.finish_at(516);
    filter(hwnd);
}

fn replace_options(hwnd: HWND, id: usize, names: &[String], selected: Option<usize>) {
    unsafe {
        let combo = GetDlgItem(hwnd, id as i32);
        SendMessageW(combo, CB_RESETCONTENT, 0, 0);
        for name in names {
            SendMessageW(combo, CB_ADDSTRING, 0, wide(name).as_ptr() as isize);
        }
        SendMessageW(combo, CB_SETCURSEL, selected.unwrap_or(usize::MAX), 0);
        EnableWindow(combo, i32::from(!names.is_empty()));
        InvalidateRect(combo, null(), 0);
    }
}

pub(super) fn filter(hwnd: HWND) {
    let query = field(hwnd, FILTER).trim().to_lowercase();
    let Some((names, selected)) = PANEL.with_borrow_mut(|p| {
        let picker = p.font_picker.as_mut()?;
        picker.visible = picker
            .families
            .iter()
            .enumerate()
            .filter_map(|(index, name)| name.to_lowercase().contains(&query).then_some(index))
            .collect();
        let mut selected = picker
            .visible
            .iter()
            .position(|&index| index == picker.family);
        if selected.is_none()
            && let Some(&family) = picker.visible.first()
        {
            picker.select_family(family, FontFace::default(), p.english);
            selected = Some(0);
        }
        Some((
            picker
                .visible
                .iter()
                .map(|&i| picker.families[i].clone())
                .collect::<Vec<_>>(),
            selected,
        ))
    }) else {
        return;
    };
    replace_options(hwnd, FAMILY, &names, selected);
    unsafe {
        EnableWindow(
            GetDlgItem(hwnd, APPLY as i32),
            i32::from(selected.is_some()),
        )
    };
    refresh_faces(hwnd);
}

fn refresh_faces(hwnd: HWND) {
    let (names, selected) = PANEL.with_borrow(|p| {
        let picker = p.font_picker.as_ref().unwrap();
        let names = picker
            .faces
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        (names, picker.face)
    });
    replace_options(hwnd, FACE, &names, Some(selected));
    update_preview(hwnd);
}

pub(super) fn selection(hwnd: HWND, id: usize) {
    let selected = unsafe { SendMessageW(GetDlgItem(hwnd, id as i32), CB_GETCURSEL, 0, 0) };
    let Ok(selected) = usize::try_from(selected) else {
        return;
    };
    let refresh = PANEL.with_borrow_mut(|p| {
        let Some(picker) = &mut p.font_picker else {
            return false;
        };
        match id {
            FAMILY => {
                if let Some(&family) = picker.visible.get(selected) {
                    picker.select_family(family, FontFace::default(), p.english);
                    return true;
                }
            }
            FACE => picker.face = selected,
            SCRIPT => picker.script = selected.min(SCRIPTS.len() - 1),
            _ => {}
        }
        false
    });
    if refresh {
        refresh_faces(hwnd);
    } else {
        update_preview(hwnd);
    }
}

fn read_size(hwnd: HWND) -> Option<i32> {
    field(hwnd, SIZE)
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|size| FONT_STEPPER.range.contains(size))
}

pub(super) fn step(hwnd: HWND, button: usize) {
    let size = PANEL.with_borrow(|p| p.font_picker.as_ref().map(|picker| picker.size));
    if let Some(size) = size {
        set_field(
            hwnd,
            SIZE as i32,
            &FONT_STEPPER.stepped(size, button).to_string(),
        );
    }
}

pub(super) fn update_preview(hwnd: HWND) {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let input_size = read_size(hwnd);
    let Some((font, sample, previous, size)) = PANEL.with_borrow_mut(|p| {
        let picker = p.font_picker.as_mut()?;
        if let Some(size) = input_size {
            picker.size = size;
        }
        let font = super::super::font::preview_font(
            &picker.families[picker.family],
            picker.face(),
            picker.size,
            dpi,
        );
        let old = std::mem::replace(&mut p.preview_font, font);
        Some((font, SCRIPTS[picker.script].2, old, picker.size))
    }) else {
        return;
    };
    let preview = unsafe { GetDlgItem(hwnd, PREVIEW as i32) };
    unsafe {
        SendMessageW(preview, WM_SETFONT, font as usize, 0);
        if !previous.is_null() {
            DeleteObject(previous);
        }
    }
    set_label(hwnd, PREVIEW as i32, sample);
    unsafe { InvalidateRect(preview, null(), 0) };
    FONT_STEPPER.sync(hwnd, size);
}

pub(super) fn apply(state: &State, hwnd: HWND) -> Result<(), String> {
    let size = read_size(hwnd).ok_or_else(|| tr("字号必须在 8 至 72 DIP 之间。").to_owned())?;
    let Some((family, face, script)) = PANEL.with_borrow(|p| {
        let picker = p.font_picker.as_ref()?;
        (!picker.visible.is_empty()).then(|| {
            (
                picker.families[picker.family].clone(),
                picker.face(),
                SCRIPTS[picker.script].0,
            )
        })
    }) else {
        return Ok(());
    };
    change(state, |settings| {
        let prefs = &mut settings.preferences;
        prefs.font = Font::System(family);
        prefs.search_font_face = face;
        prefs.search_font_size = size;
        prefs.font_preview_script = script;
    })?;
    state.reload_file_font();
    state.apply_font();
    Ok(())
}

pub(super) fn more(hwnd: HWND) -> Result<(), String> {
    let status = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            hwnd,
            w!("open"),
            w!("ms-settings:fonts"),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if status > 32 {
        Ok(())
    } else {
        Err(tr("无法打开 Windows 字体设置。").into())
    }
}
