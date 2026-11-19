use crate::engine::{DEFAULT_ENGINE, Engine, EngineError, built_in_engines, trim};
use crate::shortcut::Shortcut;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
            excluded_apps: Vec::new(),
            record_history: false,
            start_with_windows: false,
            show_on_manual_launch: true,
            close_to_tray: true,
            show_engine_names: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub schema: u32,
    pub preferences: Preferences,
    pub custom_engines: Vec<Engine>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            preferences: Preferences::default(),
            custom_engines: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnginePack {
    pub schema: u32,
    pub engines: Vec<Engine>,
    pub pins: Vec<String>,
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
        engines.extend(self.custom_engines.iter().cloned());
        engines
    }

    pub fn normalize(&mut self) -> Result<(), SettingsError> {
        if self.schema != SCHEMA {
            return Err(SettingsError::Schema(self.schema));
        }
        let built_ins = built_in_engines();
        let mut ids: HashSet<_> = built_ins.iter().map(|e| e.id.as_str()).collect();
        let mut aliases: HashSet<_> = built_ins.iter().map(|e| e.alias.as_str()).collect();
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
            if !engine.alias.is_empty() && !aliases.insert(&engine.alias) {
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
        if (main != Shortcut::default() && !main.is_combination())
            || selection.is_some_and(|s| !s.is_combination() || s == main)
            || clipboard.is_some_and(|s| !s.is_combination() || s == main || Some(s) == selection)
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
        let previous = std::mem::replace(&mut self.preferences.pins, pack.pins);
        self.preferences.pins.extend(previous);
        Ok(())
    }
}
