use crate::engine::{DEFAULT_ENGINE, Engine, EngineError, built_in_engines, normalize_alias, trim};
use crate::shortcut::Shortcut;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use thiserror::Error;

pub const SCHEMA: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    System,
    Chinese,
    English,
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
    pub default_engine: String,
    pub pins: Vec<String>,
    pub browser: Option<PathBuf>,
    pub main_shortcut: Shortcut,
    pub selection: Option<SelectionBinding>,
    pub clipboard_shortcut: Option<Shortcut>,
    pub ocr_enabled: bool,
    pub ocr_model_dir: Option<PathBuf>,
    pub ocr_shortcut: Option<Shortcut>,
    pub excluded_apps: Vec<String>,
    pub record_history: bool,
    pub start_with_windows: bool,
    pub show_on_manual_launch: bool,
    pub close_to_tray: bool,
    pub show_engine_names: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: Language::System,
            font: Font::Default,
            default_engine: DEFAULT_ENGINE.into(),
            pins: Vec::new(),
            browser: None,
            main_shortcut: Shortcut::default(),
            selection: None,
            clipboard_shortcut: None,
            ocr_enabled: false,
            ocr_model_dir: None,
            ocr_shortcut: Some(Shortcut {
                modifiers: crate::shortcut::ALT | crate::shortcut::SHIFT,
                key: b'O' as u32,
            }),
            excluded_apps: Vec::new(),
            record_history: false,
            start_with_windows: false,
            show_on_manual_launch: true,
            close_to_tray: true,
            show_engine_names: true,
        }
    }
}

impl Preferences {
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            preferences: Preferences::default(),
            custom_engines: Vec::new(),
            built_in_aliases: BTreeMap::new(),
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
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("unsupported settings version {0}")]
    Schema(u32),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("unknown engine {0}")]
    EngineReference(String),
    #[error("invalid or duplicate keyboard shortcut")]
    Shortcut,
    #[error("invalid excluded executable name")]
    ExcludedApp,
}

impl Settings {
    pub fn engines(&self) -> Vec<Engine> {
        let mut engines = built_in_engines();
        for engine in &mut engines {
            if let Some(alias) = self.built_in_aliases.get(&engine.id) {
                engine.alias.clone_from(alias);
            }
        }
        engines.extend(self.custom_engines.iter().cloned());
        engines
    }

    pub fn normalize(&mut self) -> Result<(), SettingsError> {
        if self.schema != SCHEMA {
            return Err(SettingsError::Schema(self.schema));
        }
        let mut built_ins = built_in_engines();
        for (id, alias) in &mut self.built_in_aliases {
            let engine = built_ins
                .iter_mut()
                .find(|engine| engine.id == *id)
                .ok_or_else(|| SettingsError::EngineReference(id.clone()))?;
            *alias = normalize_alias(alias)?;
            engine.alias.clone_from(alias);
        }
        let mut ids: HashSet<_> = built_ins.iter().map(|e| e.id.as_str()).collect();
        let mut aliases = HashSet::new();
        for engine in &built_ins {
            if !engine.alias.is_empty() && !aliases.insert(engine.alias.as_str()) {
                return Err(EngineError::DuplicateAlias.into());
            }
        }
        for engine in &mut self.custom_engines {
            engine.normalize()?;
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
        let main = prefs.main_shortcut;
        let selection = prefs.selection.as_ref().map(|binding| binding.shortcut);
        let clipboard = prefs.clipboard_shortcut;
        let ocr = prefs.ocr_shortcut;
        if (main != Shortcut::default() && !main.is_combination())
            || selection.is_some_and(|s| !s.is_combination() || s == main)
            || clipboard.is_some_and(|s| !s.is_combination() || s == main || Some(s) == selection)
            || (cfg!(feature = "ocr")
                && prefs.active_ocr_model_dir().is_some()
                && ocr.is_some_and(|s| {
                    !s.is_combination() || s == main || Some(s) == selection || Some(s) == clipboard
                }))
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
        let previous = std::mem::replace(&mut self.preferences.pins, pack.pins);
        self.preferences.pins.extend(previous);
        Ok(())
    }
}
