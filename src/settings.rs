use crate::engine::{DEFAULT_ENGINE, Engine, EngineError, built_in_engines, normalize_alias, trim};
use crate::shortcut::Shortcut;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use thiserror::Error;

pub const SCHEMA: u32 = 1;
pub const DEFAULT_SEARCH_ENGINE_ICON_SIZE: i32 = 20;
pub const MAX_SEARCH_ENGINE_ICON_SIZE: i32 = 36;
pub const MAX_SEARCH_PADDING: i32 = 64;
pub const DEFAULT_SEARCH_WIDTH: i32 = 580;
pub const MIN_SEARCH_WIDTH: i32 = 320;
pub const MAX_SEARCH_WIDTH: i32 = 1600;
pub const DEFAULT_SEARCH_FONT_SIZE: i32 = 20;
pub const MIN_SEARCH_FONT_SIZE: i32 = 8;
pub const MAX_SEARCH_FONT_SIZE: i32 = 72;
pub const DEFAULT_BACKGROUND_BLUR_PERCENT: i32 = 50;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenBackground {
    Dim,
    #[default]
    System,
    Gaussian,
}

fn read_screen_background<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<ScreenBackground, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Mode(ScreenBackground),
        Enabled(bool),
    }

    Ok(match Value::deserialize(deserializer)? {
        Value::Mode(mode) => mode,
        Value::Enabled(true) => ScreenBackground::System,
        Value::Enabled(false) => ScreenBackground::Dim,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    System,
    Chinese,
    English,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMaterial {
    #[default]
    Acrylic,
    Mica,
    Solid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchTone {
    #[default]
    System,
    Light,
    Dark,
}

impl SearchTone {
    pub fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Self::System => system_dark,
            Self::Light => false,
            Self::Dark => true,
        }
    }
}

pub const DEFAULT_HISTORY_HIGHLIGHT: u32 = 0x1f75db;
pub const DEFAULT_CORNER_RADIUS: i32 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryMaterial {
    Acrylic,
    Mica,
    #[default]
    Solid,
    FollowSearch,
}

impl HistoryMaterial {
    pub fn resolve(self, search: SearchMaterial) -> SearchMaterial {
        match self {
            Self::Acrylic => SearchMaterial::Acrylic,
            Self::Mica => SearchMaterial::Mica,
            Self::Solid => SearchMaterial::Solid,
            Self::FollowSearch => search,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryPopup {
    pub enabled: bool,
    pub show_shortcuts: bool,
    pub material: HistoryMaterial,
    pub tone: SearchTone,
    pub light_color: u32,
    pub dark_color: u32,
    pub acrylic_tint_percent: i32,
    pub mica_tint_percent: i32,
    pub highlight_color: Option<u32>,
    pub highlight_transparency: i32,
    pub corner_radius: i32,
}

impl Default for HistoryPopup {
    fn default() -> Self {
        Self {
            enabled: true,
            show_shortcuts: true,
            material: HistoryMaterial::Solid,
            tone: SearchTone::System,
            light_color: 0xf7f8fb,
            dark_color: 0x222831,
            acrylic_tint_percent: 68,
            mica_tint_percent: 16,
            highlight_color: None,
            highlight_transparency: 81,
            corner_radius: DEFAULT_CORNER_RADIUS,
        }
    }
}

impl HistoryPopup {
    pub fn tint(&self, dark: bool) -> u32 {
        if dark {
            self.dark_color
        } else {
            self.light_color
        }
    }

    pub fn tint_percent(&self, material: SearchMaterial) -> i32 {
        match material {
            SearchMaterial::Acrylic => self.acrylic_tint_percent,
            SearchMaterial::Mica => self.mica_tint_percent,
            SearchMaterial::Solid => 100,
        }
    }
}

#[derive(Clone, Copy)]
pub struct PopupBackground {
    pub material: SearchMaterial,
    pub dark: bool,
    pub color: u32,
    pub tint_percent: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "source", content = "value")]
pub enum Font {
    #[default]
    Default,
    System(String),
    File(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionBinding {
    pub shortcut: Shortcut,
    pub engine: String,
    pub preview: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub language: Language,
    pub font: Font,
    pub search_font_size: i32,
    pub search_width: i32,
    pub search_padding: SearchPadding,
    pub search_corner_radius: i32,
    pub search_font_face: FontFace,
    pub font_preview_script: FontPreviewScript,
    pub search_material: SearchMaterial,
    pub search_tone: SearchTone,
    pub search_light_color: u32,
    pub search_dark_color: u32,
    pub search_text_color: Option<u32>,
    pub search_text_transparency: i32,
    pub search_mono_icon_color: Option<u32>,
    pub search_mono_icon_transparency: i32,
    pub recent_search_colors: Vec<u32>,
    pub acrylic_tint_percent: i32,
    pub mica_tint_percent: i32,
    #[serde(alias = "screen_blur", deserialize_with = "read_screen_background")]
    pub screen_background: ScreenBackground,
    pub background_blur_percent: i32,
    pub default_engine: String,
    pub pins: Vec<String>,
    pub browser: Option<PathBuf>,
    pub main_shortcut: Option<Shortcut>,
    pub selection: Option<SelectionBinding>,
    pub clipboard_shortcut: Option<Shortcut>,
    pub ocr_enabled: bool,
    pub ocr_model_dir: Option<PathBuf>,
    pub ocr_shortcut: Option<Shortcut>,
    pub image_search_shortcut: Option<Shortcut>,
    pub image_search_action: ImageSearchAction,
    pub excluded_apps: Vec<String>,
    pub record_history: bool,
    pub history_popup: HistoryPopup,
    pub start_with_windows: bool,
    pub show_on_manual_launch: bool,
    pub close_to_tray: bool,
    pub show_engine_names: bool,
    pub show_search_engine_name: bool,
    pub search_engine_icon_size: i32,
    pub multicolor_engine_icons: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: Language::System,
            font: Font::Default,
            search_font_size: DEFAULT_SEARCH_FONT_SIZE,
            search_width: DEFAULT_SEARCH_WIDTH,
            search_padding: SearchPadding::default(),
            search_corner_radius: DEFAULT_CORNER_RADIUS,
            search_font_face: FontFace::default(),
            font_preview_script: FontPreviewScript::default(),
            search_material: SearchMaterial::default(),
            search_tone: SearchTone::default(),
            search_light_color: 0xffffff,
            search_dark_color: 0,
            search_text_color: None,
            search_text_transparency: 0,
            search_mono_icon_color: None,
            search_mono_icon_transparency: 0,
            recent_search_colors: Vec::new(),
            acrylic_tint_percent: 68,
            mica_tint_percent: 16,
            screen_background: ScreenBackground::System,
            background_blur_percent: DEFAULT_BACKGROUND_BLUR_PERCENT,
            default_engine: DEFAULT_ENGINE.into(),
            pins: Vec::new(),
            browser: None,
            main_shortcut: Some(Shortcut::default()),
            selection: None,
            clipboard_shortcut: None,
            ocr_enabled: false,
            ocr_model_dir: None,
            ocr_shortcut: Some(Shortcut {
                modifiers: crate::shortcut::ALT | crate::shortcut::SHIFT,
                key: b'O' as u32,
            }),
            image_search_shortcut: None,
            image_search_action: ImageSearchAction::Choose,
            excluded_apps: Vec::new(),
            record_history: false,
            history_popup: HistoryPopup::default(),
            start_with_windows: false,
            show_on_manual_launch: true,
            close_to_tray: true,
            show_engine_names: true,
            show_search_engine_name: false,
            search_engine_icon_size: DEFAULT_SEARCH_ENGINE_ICON_SIZE,
            multicolor_engine_icons: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSearchAction {
    #[default]
    Choose,
    Both,
    Yandex,
    GoogleLens,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchPadding {
    pub top: i32,
    pub bottom: i32,
    pub left: i32,
    pub right: i32,
}

impl Default for SearchPadding {
    fn default() -> Self {
        Self {
            top: 8,
            bottom: 8,
            left: 20,
            right: 12,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontFace {
    pub weight: u16,
    pub stretch: u8,
    pub style: u8,
}

impl Default for FontFace {
    fn default() -> Self {
        Self {
            weight: 400,
            stretch: 5,
            style: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontPreviewScript {
    #[default]
    SimplifiedChinese,
    TraditionalChinese,
    Latin,
    Japanese,
    Korean,
}

impl Preferences {
    pub fn history_background(&self, system_dark: bool) -> PopupBackground {
        let history = &self.history_popup;
        let material = history.material.resolve(self.search_material);
        if history.material == HistoryMaterial::FollowSearch {
            let dark = self.search_tone.is_dark(system_dark);
            PopupBackground {
                material,
                dark,
                color: self.search_tint(dark),
                tint_percent: self.search_tint_percent(),
            }
        } else {
            let dark = history.tone.is_dark(system_dark);
            PopupBackground {
                material,
                dark,
                color: history.tint(dark),
                tint_percent: history.tint_percent(material),
            }
        }
    }

    pub fn search_tint(&self, dark: bool) -> u32 {
        if dark {
            self.search_dark_color
        } else {
            self.search_light_color
        }
    }

    pub fn search_tint_percent(&self) -> i32 {
        match self.search_material {
            SearchMaterial::Acrylic => self.acrylic_tint_percent,
            SearchMaterial::Mica => self.mica_tint_percent,
            SearchMaterial::Solid => 100,
        }
    }

    pub fn search_text_opacity(&self) -> f32 {
        (100 - self.search_text_transparency) as f32 / 100.0
    }

    pub fn search_mono_icon_opacity(&self) -> f32 {
        (100 - self.search_mono_icon_transparency) as f32 / 100.0
    }

    pub fn active_ocr_model_dir(&self) -> Option<&std::path::Path> {
        self.ocr_model_dir.as_deref().filter(|_| self.ocr_enabled)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub schema: u32,
    pub preferences: Preferences,
    pub custom_engines: Vec<Engine>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub built_in_aliases: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub built_in_overrides: BTreeMap<String, Engine>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub engine_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_engines: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            preferences: Preferences::default(),
            custom_engines: Vec::new(),
            built_in_aliases: BTreeMap::new(),
            built_in_overrides: BTreeMap::new(),
            engine_order: Vec::new(),
            disabled_engines: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnginePack {
    pub schema: u32,
    pub engines: Vec<Engine>,
    pub pins: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub built_in_aliases: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub built_in_overrides: BTreeMap<String, Engine>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub engine_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_engines: Vec<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("unsupported settings version {0}")]
    Schema(u32),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("unknown engine {0}")]
    EngineReference(String),
    #[error("at least one search engine must remain enabled")]
    NoEnabledEngine,
    #[error("invalid or duplicate keyboard shortcut")]
    Shortcut,
    #[error("invalid excluded executable name")]
    ExcludedApp,
}

impl Settings {
    fn effective_built_ins(&self) -> Vec<Engine> {
        let mut engines = built_in_engines();
        for engine in &mut engines {
            if let Some(override_engine) = self.built_in_overrides.get(&engine.id) {
                engine.clone_from(override_engine);
            }
            if let Some(alias) = self.built_in_aliases.get(&engine.id) {
                engine.alias.clone_from(alias);
            }
        }
        // The new default alias must not invalidate an existing configuration.
        if !self.built_in_aliases.contains_key("x")
            && !self.built_in_overrides.contains_key("x")
            && engines
                .iter()
                .chain(&self.custom_engines)
                .any(|e| e.id != "x" && e.alias == "x")
        {
            if let Some(engine) = engines.iter_mut().find(|e| e.id == "x") {
                engine.alias.clear();
            }
        }
        engines
    }

    pub fn engines(&self) -> Vec<Engine> {
        let mut engines = self.effective_built_ins();
        engines.extend(self.custom_engines.iter().cloned());
        let positions: BTreeMap<_, _> = self.engine_order.iter().zip(0..).collect();
        engines.sort_by_key(|engine| positions.get(&engine.id).copied().unwrap_or(usize::MAX));
        engines
    }

    pub fn engine_enabled(&self, id: &str) -> bool {
        !self.disabled_engines.iter().any(|disabled| disabled == id)
    }

    pub fn normalize(&mut self) -> Result<(), SettingsError> {
        if self.schema != SCHEMA {
            return Err(SettingsError::Schema(self.schema));
        }
        let built_ins = built_in_engines();
        for (id, engine) in &mut self.built_in_overrides {
            if engine.id != *id || !built_ins.iter().any(|e| e.id == *id) {
                return Err(SettingsError::EngineReference(id.clone()));
            }
            engine.normalize()?;
        }
        for (id, alias) in &mut self.built_in_aliases {
            if !built_ins.iter().any(|engine| engine.id == *id) {
                return Err(SettingsError::EngineReference(id.clone()));
            }
            *alias = normalize_alias(alias)?;
        }
        for engine in &mut self.custom_engines {
            engine.normalize()?;
        }
        let built_ins = self.effective_built_ins();
        let mut ids: HashSet<_> = built_ins.iter().map(|e| e.id.as_str()).collect();
        let mut aliases = HashSet::new();
        for engine in &built_ins {
            if !engine.alias.is_empty() && !aliases.insert(engine.alias.as_str()) {
                return Err(EngineError::DuplicateAlias.into());
            }
        }
        for engine in &mut self.custom_engines {
            let valid_id = engine.id.strip_prefix("custom-").is_some_and(|id| {
                !id.is_empty()
                    && id.len() <= 64
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            });
            if !valid_id || !ids.insert(&engine.id) {
                return Err(EngineError::Id.into());
            }
            if !engine.alias.is_empty() && !aliases.insert(engine.alias.as_str()) {
                return Err(EngineError::DuplicateAlias.into());
            }
        }
        let prefs = &mut self.preferences;
        prefs.search_font_size = prefs
            .search_font_size
            .clamp(MIN_SEARCH_FONT_SIZE, MAX_SEARCH_FONT_SIZE);
        prefs.search_width = prefs.search_width.clamp(MIN_SEARCH_WIDTH, MAX_SEARCH_WIDTH);
        let padding = &mut prefs.search_padding;
        for side in [
            &mut padding.top,
            &mut padding.bottom,
            &mut padding.left,
            &mut padding.right,
        ] {
            *side = (*side).clamp(0, MAX_SEARCH_PADDING);
        }
        prefs.search_light_color &= 0xffffff;
        prefs.search_dark_color &= 0xffffff;
        prefs.search_text_color = prefs.search_text_color.map(|rgb| rgb & 0xffffff);
        prefs.search_mono_icon_color = prefs.search_mono_icon_color.map(|rgb| rgb & 0xffffff);
        prefs.search_text_transparency = prefs.search_text_transparency.clamp(0, 100);
        prefs.search_mono_icon_transparency = prefs.search_mono_icon_transparency.clamp(0, 100);
        let mut recent = HashSet::new();
        prefs
            .recent_search_colors
            .retain(|&rgb| rgb <= 0xffffff && recent.insert(rgb));
        prefs.recent_search_colors.truncate(8);
        prefs.acrylic_tint_percent = prefs.acrylic_tint_percent.clamp(0, 100);
        prefs.mica_tint_percent = prefs.mica_tint_percent.clamp(0, 100);
        prefs.background_blur_percent = prefs.background_blur_percent.clamp(0, 100);
        let history = &mut prefs.history_popup;
        prefs.search_corner_radius = prefs.search_corner_radius.max(0);
        history.corner_radius = history.corner_radius.max(0);
        history.light_color &= 0xffffff;
        history.dark_color &= 0xffffff;
        history.highlight_color = history.highlight_color.map(|rgb| rgb & 0xffffff);
        history.acrylic_tint_percent = history.acrylic_tint_percent.clamp(0, 100);
        history.mica_tint_percent = history.mica_tint_percent.clamp(0, 100);
        history.highlight_transparency = history.highlight_transparency.clamp(0, 100);
        let face = &mut prefs.search_font_face;
        face.weight = face.weight.clamp(1, 999);
        face.stretch = face.stretch.clamp(1, 9);
        face.style = face.style.min(2);
        if prefs.search_engine_icon_size <= 0 {
            prefs.search_engine_icon_size = DEFAULT_SEARCH_ENGINE_ICON_SIZE;
        }
        prefs.search_engine_icon_size = prefs
            .search_engine_icon_size
            .min(MAX_SEARCH_ENGINE_ICON_SIZE);
        let selection_engine = prefs.selection.iter().map(|binding| &binding.engine);
        for id in std::iter::once(&prefs.default_engine)
            .chain(&prefs.pins)
            .chain(selection_engine)
        {
            if !ids.contains(id.as_str()) {
                return Err(SettingsError::EngineReference(id.clone()));
            }
        }
        let mut seen = HashSet::new();
        prefs.pins.retain(|id| seen.insert(id.clone()));
        for list in [&mut self.engine_order, &mut self.disabled_engines] {
            seen.clear();
            list.retain(|id| ids.contains(id.as_str()) && seen.insert(id.clone()));
        }
        let fallback = self
            .engines()
            .into_iter()
            .find(|e| self.engine_enabled(&e.id))
            .ok_or(SettingsError::NoEnabledEngine)?
            .id;
        let default_enabled = self.engine_enabled(&self.preferences.default_engine);
        let selection_enabled = self
            .preferences
            .selection
            .as_ref()
            .is_none_or(|binding| self.engine_enabled(&binding.engine));
        let prefs = &mut self.preferences;
        if !default_enabled {
            prefs.default_engine = fallback;
        }
        if !selection_enabled {
            prefs
                .selection
                .as_mut()
                .unwrap()
                .engine
                .clone_from(&prefs.default_engine);
        }
        let main = prefs.main_shortcut;
        let selection = prefs.selection.as_ref().map(|binding| binding.shortcut);
        let clipboard = prefs.clipboard_shortcut;
        let ocr = prefs
            .ocr_shortcut
            .filter(|_| cfg!(feature = "ocr") && prefs.active_ocr_model_dir().is_some());
        if main.is_some_and(|s| s != Shortcut::default() && !s.is_combination())
            || selection.is_some_and(|s| !s.is_combination() || Some(s) == main)
            || clipboard
                .is_some_and(|s| !s.is_combination() || Some(s) == main || Some(s) == selection)
            || ocr.is_some_and(|s| {
                !s.is_combination()
                    || Some(s) == main
                    || Some(s) == selection
                    || Some(s) == clipboard
            })
            || prefs.image_search_shortcut.is_some_and(|s| {
                !s.is_combination() || [main, selection, clipboard, ocr].contains(&Some(s))
            })
        {
            return Err(SettingsError::Shortcut);
        }
        seen.clear();
        for name in &mut prefs.excluded_apps {
            *name = trim(name).to_ascii_lowercase();
            if name.len() <= 4
                || name.encode_utf16().count() > 255
                || !name.ends_with(".exe")
                || name.contains([
                    '\\', '/', ':', '*', '?', '"', '<', '>', '|', ',', '\t', '\r', '\n', '\0',
                ])
            {
                return Err(SettingsError::ExcludedApp);
            }
        }
        prefs.excluded_apps.retain(|name| seen.insert(name.clone()));
        Ok(())
    }

    pub fn engine_pack(&self) -> EnginePack {
        EnginePack {
            schema: SCHEMA,
            engines: self.custom_engines.clone(),
            pins: self.preferences.pins.clone(),
            built_in_aliases: self.built_in_aliases.clone(),
            built_in_overrides: self.built_in_overrides.clone(),
            engine_order: self.engine_order.clone(),
            disabled_engines: self.disabled_engines.clone(),
        }
    }

    pub(crate) fn merge_engines(&mut self, pack: EnginePack) -> Result<(), SettingsError> {
        if pack.schema != SCHEMA {
            return Err(SettingsError::Schema(pack.schema));
        }
        let mut engines = pack.engines;
        let incoming: HashSet<_> = engines.iter().map(|engine| engine.id.clone()).collect();
        engines.extend(
            self.custom_engines
                .drain(..)
                .filter(|engine| !incoming.contains(&engine.id)),
        );
        self.custom_engines = engines;
        self.built_in_aliases.extend(pack.built_in_aliases);
        self.built_in_overrides.extend(pack.built_in_overrides);
        let mut order = pack.engine_order;
        let ordered: HashSet<_> = order.iter().cloned().collect();
        order.extend(
            self.engine_order
                .drain(..)
                .filter(|id| !ordered.contains(id)),
        );
        self.engine_order = order;
        self.disabled_engines
            .retain(|id| !incoming.contains(id) && !ordered.contains(id));
        self.disabled_engines.extend(pack.disabled_engines);
        let previous = std::mem::replace(&mut self.preferences.pins, pack.pins);
        self.preferences.pins.extend(previous);
        Ok(())
    }
}
