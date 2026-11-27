use crate::engine::{
    DEFAULT_ENGINE, Engine, EngineError, alias_prefix, is_search_template, is_web_url, search_url,
    trim,
};
use crate::settings::{Settings, SettingsError};
use crate::storage::{
    SettingsStore, StorageError, import_engines, import_settings, read_json, write_json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Typed,
    Selection,
    Clipboard,
    History,
    Ocr,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: u64,
    pub timestamp_ms: u64,
    pub query: String,
    pub engine_id: String,
    pub engine_name: String,
    pub origin: Origin,
    pub source: String,
}

pub trait Browser {
    fn open(&mut self, url: &str, executable: Option<&Path>) -> io::Result<()>;
}

pub struct History {
    path: PathBuf,
    records: Option<Vec<Record>>,
}

impl History {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            records: None,
        }
    }

    pub fn release_cache(&mut self) {
        self.records = None;
    }

    pub fn records(&mut self) -> Result<&[Record], StorageError> {
        if self.records.is_none() {
            self.records = Some(match read_json(&self.path) {
                Err(StorageError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                    Vec::new()
                }
                other => other?,
            });
        }
        Ok(self.records.as_deref().unwrap_or_default())
    }

    fn append(&mut self, mut record: Record) -> Result<(), StorageError> {
        let mut updated = self.records()?.to_vec();
        record.id = next_history_id(&updated)?;
        updated.insert(0, record);
        write_json(&self.path, &updated)?;
        self.records = Some(updated);
        Ok(())
    }

    pub fn page(
        &mut self,
        keyword: &str,
        source: &str,
        page: usize,
    ) -> Result<Vec<Record>, StorageError> {
        Ok(self
            .matching(keyword, source)?
            .skip(page.saturating_mul(24))
            .take(24)
            .cloned()
            .collect())
    }

    pub fn count(&mut self, keyword: &str, source: &str) -> Result<usize, StorageError> {
        Ok(self.matching(keyword, source)?.count())
    }

    fn matching(
        &mut self,
        keyword: &str,
        source: &str,
    ) -> Result<impl Iterator<Item = &Record>, StorageError> {
        let keyword = trim(keyword).to_lowercase();
        let source = trim(source).to_lowercase();
        Ok(self.records()?.iter().filter(move |record| {
            (keyword.is_empty() || record.query.to_lowercase().contains(&keyword))
                && (source.is_empty() || record.source.to_lowercase().contains(&source))
        }))
    }

    pub fn restore(&mut self, mut record: Record) -> Result<(), StorageError> {
        let mut updated = self.records()?.to_vec();
        if updated.iter().any(|entry| entry.id == record.id) {
            record.id = next_history_id(&updated)?;
        }
        let index = updated
            .iter()
            .position(|entry| entry.timestamp_ms < record.timestamp_ms)
            .unwrap_or(updated.len());
        updated.insert(index, record);
        write_json(&self.path, &updated)?;
        self.records = Some(updated);
        Ok(())
    }

    pub fn delete(&mut self, id: u64) -> Result<bool, StorageError> {
        let mut updated = self.records()?.to_vec();
        let Some(index) = updated.iter().position(|record| record.id == id) else {
            return Ok(false);
        };
        updated.remove(index);
        write_json(&self.path, &updated)?;
        self.records = Some(updated);
        Ok(true)
    }
}

fn next_history_id(records: &[Record]) -> io::Result<u64> {
    records
        .iter()
        .map(|record| record.id)
        .max()
        .unwrap_or_default()
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "history ID exhausted"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    pub engine: Option<String>,
    pub origin: Origin,
    pub source: String,
}

#[derive(Debug)]
pub enum SearchOutcome {
    Empty,
    Opened { history_error: Option<StorageError> },
    Failed { query: Query, error: io::Error },
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Engine(#[from] EngineError),
}

pub struct Application<S, B> {
    settings: Settings,
    engines: Vec<Engine>,
    all_engines: Option<Vec<Engine>>,
    store: S,
    browser: B,
    history: History,
    last_search: Option<Query>,
}

impl<S: SettingsStore, B: Browser> Application<S, B> {
    pub fn new(
        mut settings: Settings,
        store: S,
        browser: B,
        history: History,
    ) -> Result<Self, SettingsError> {
        settings.normalize()?;
        let (engines, all_engines) = Self::engine_lists(&settings);
        Ok(Self {
            settings,
            engines,
            all_engines,
            store,
            browser,
            history,
            last_search: None,
        })
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    pub fn engines(&self) -> &[Engine] {
        &self.engines
    }
    pub fn all_engines(&self) -> &[Engine] {
        self.all_engines.as_deref().unwrap_or(&self.engines)
    }
    pub fn last_query(&self) -> &str {
        self.last_search().map_or("", |search| search.text.as_str())
    }
    pub fn last_search(&self) -> Option<&Query> {
        self.last_search.as_ref()
    }
    pub fn history(&mut self) -> &mut History {
        &mut self.history
    }

    pub fn history_suggestions(&mut self, keyword: &str) -> Result<Vec<Record>, StorageError> {
        if !self.settings.preferences.history_popup.enabled || trim(keyword).is_empty() {
            return Ok(Vec::new());
        }
        let mut seen = HashSet::new();
        Ok(self
            .history
            .matching(keyword, "")?
            .filter(|record| seen.insert((record.query.as_str(), record.engine_id.as_str())))
            .take(8)
            .cloned()
            .collect())
    }

    pub fn change_settings(
        &mut self,
        update: impl FnOnce(&mut Settings),
    ) -> Result<(), UpdateError> {
        let mut settings = self.settings.clone();
        update(&mut settings);
        self.replace_settings(settings)
    }

    fn replace_settings(&mut self, mut settings: Settings) -> Result<(), UpdateError> {
        settings.normalize()?;
        self.store.save(&settings)?;
        (self.engines, self.all_engines) = Self::engine_lists(&settings);
        self.settings = settings;
        Ok(())
    }

    fn engine_lists(settings: &Settings) -> (Vec<Engine>, Option<Vec<Engine>>) {
        let all = settings.engines();
        if settings.disabled_engines.is_empty() {
            return (all, None);
        }
        let enabled = all
            .iter()
            .filter(|engine| settings.engine_enabled(&engine.id))
            .cloned()
            .collect();
        (enabled, Some(all))
    }

    pub fn import_settings(&mut self, path: &Path) -> Result<(), UpdateError> {
        self.replace_settings(import_settings(path)?)
    }

    pub fn export_settings(&self, path: &Path) -> Result<(), StorageError> {
        write_json(path, &self.settings)
    }

    pub fn import_engine_pack(&mut self, path: &Path) -> Result<(), UpdateError> {
        self.merge_engine_pack(import_engines(path)?)
    }

    pub fn merge_engine_pack(
        &mut self,
        pack: crate::settings::EnginePack,
    ) -> Result<(), UpdateError> {
        let mut settings = self.settings.clone();
        settings.merge_engines(pack)?;
        self.replace_settings(settings)
    }

    pub fn export_engine_pack(&self, path: &Path) -> Result<(), StorageError> {
        write_json(path, &self.settings.engine_pack())
    }

    pub fn set_built_in_alias(&mut self, id: &str, alias: String) -> Result<(), UpdateError> {
        self.change_settings(|settings| {
            settings.built_in_aliases.insert(id.to_owned(), alias);
        })
    }

    pub fn validate_engine(&self, engine: &mut Engine) -> Result<(), EngineError> {
        engine.normalize()?;
        if !engine.alias.is_empty()
            && self
                .all_engines()
                .iter()
                .any(|other| other.id != engine.id && other.alias == engine.alias)
        {
            return Err(EngineError::DuplicateAlias);
        }
        Ok(())
    }

    pub fn save_engine(&mut self, mut engine: Engine) -> Result<String, UpdateError> {
        let mut settings = self.settings.clone();
        self.validate_engine(&mut engine)
            .map_err(SettingsError::from)?;
        if engine.id.is_empty() {
            let index = (1u64..)
                .find(|index| {
                    !self
                        .all_engines()
                        .iter()
                        .any(|e| e.id == format!("custom-{index}"))
                })
                .ok_or(EngineError::Unavailable)?;
            engine.id = format!("custom-{index}");
            settings.custom_engines.push(engine.clone());
        } else if let Some(mut original) = crate::engine::built_in_engines()
            .into_iter()
            .find(|original| original.id == engine.id)
        {
            settings
                .built_in_aliases
                .insert(engine.id.clone(), engine.alias.clone());
            original.alias.clone_from(&engine.alias);
            if original == engine {
                settings.built_in_overrides.remove(&engine.id);
            } else {
                settings
                    .built_in_overrides
                    .insert(engine.id.clone(), engine.clone());
            }
        } else {
            let previous = settings
                .custom_engines
                .iter_mut()
                .find(|e| e.id == engine.id)
                .ok_or(EngineError::Unavailable)?;
            *previous = engine.clone();
        }
        self.replace_settings(settings)?;
        Ok(engine.id)
    }

    pub fn delete_engine(&mut self, id: &str) -> Result<(), UpdateError> {
        if !self
            .settings
            .custom_engines
            .iter()
            .any(|engine| engine.id == id)
        {
            return Err(EngineError::Unavailable.into());
        }
        self.change_settings(|settings| {
            settings.custom_engines.retain(|engine| engine.id != id);
            let prefs = &mut settings.preferences;
            prefs.pins.retain(|pin| pin != id);
            if prefs.default_engine == id {
                prefs.default_engine = DEFAULT_ENGINE.into();
            }
            if prefs
                .selection
                .as_ref()
                .is_some_and(|binding| binding.engine == id)
            {
                prefs.selection = None;
            }
        })
    }

    pub fn move_engine(&mut self, id: &str, direction: isize) -> Result<(), UpdateError> {
        let engines = &self.settings.custom_engines;
        let index = engines
            .iter()
            .position(|engine| engine.id == id)
            .ok_or(EngineError::Unavailable)?;
        let next = index
            .checked_add_signed(direction)
            .filter(|&next| next < engines.len() && matches!(direction, -1 | 1))
            .ok_or(EngineError::Unavailable)?;
        self.change_settings(|settings| settings.custom_engines.swap(index, next))
    }

    pub fn toggle_pin(&mut self, id: &str) -> Result<(), UpdateError> {
        self.change_settings(|settings| {
            let pins = &mut settings.preferences.pins;
            if let Some(index) = pins.iter().position(|pin| pin == id) {
                pins.remove(index);
            } else {
                pins.push(id.into());
            }
        })
    }

    pub fn set_engine_enabled(&mut self, id: &str, enabled: bool) -> Result<(), UpdateError> {
        if !self.all_engines().iter().any(|engine| engine.id == id) {
            return Err(EngineError::Unavailable.into());
        }
        self.change_settings(|settings| {
            settings.disabled_engines.retain(|disabled| disabled != id);
            if !enabled {
                settings.disabled_engines.push(id.into());
            }
        })
    }

    pub fn reorder_engines(&mut self, visible_order: &[String]) -> Result<(), UpdateError> {
        let visible: std::collections::HashSet<_> = visible_order.iter().collect();
        if visible.len() != visible_order.len()
            || !visible
                .iter()
                .all(|id| self.all_engines().iter().any(|engine| &engine.id == *id))
        {
            return Err(EngineError::Unavailable.into());
        }
        let mut next = visible_order.iter();
        let order = self
            .all_engines()
            .iter()
            .map(|engine| {
                if visible.contains(&engine.id) {
                    next.next().unwrap().clone()
                } else {
                    engine.id.clone()
                }
            })
            .collect();
        self.change_settings(|settings| settings.engine_order = order)
    }

    pub fn pinned_engine(&self, index: usize) -> Option<&str> {
        self.settings
            .preferences
            .pins
            .get(index)
            .filter(|id| index < 5 && self.settings.engine_enabled(id))
            .map(String::as_str)
    }

    pub fn suggestions(&self, input: &str) -> Vec<&Engine> {
        crate::engine::suggestions(&self.engines, &self.settings.preferences.pins, input)
    }

    pub fn open_link(&mut self, input: &str) -> io::Result<bool> {
        self.open_web_link(input, false)
    }

    pub fn open_system_link(&mut self, input: &str) -> io::Result<bool> {
        self.open_web_link(input, true)
    }

    fn open_web_link(&mut self, input: &str, system: bool) -> io::Result<bool> {
        if !is_web_url(input) {
            return Ok(false);
        }
        let browser = if system {
            None
        } else {
            self.settings.preferences.browser.as_deref()
        };
        self.browser.open(trim(input), browser)?;
        Ok(true)
    }

    pub fn test_engine(&mut self, template: &str, query: &str) -> io::Result<()> {
        if !is_search_template(template) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid search template",
            ));
        }
        self.browser.open(
            &search_url(trim(template), query),
            self.settings.preferences.browser.as_deref(),
        )
    }

    pub fn search(&mut self, query: Query) -> SearchOutcome {
        self.search_using(query, false)
    }

    pub fn search_with_system_browser(&mut self, query: Query) -> SearchOutcome {
        self.search_using(query, true)
    }

    fn search_using(&mut self, query: Query, system: bool) -> SearchOutcome {
        let text = trim(&query.text);
        let engine = query
            .engine
            .as_ref()
            .and_then(|id| self.engines.iter().find(|engine| engine.id == *id));
        let alias = (query.origin == Origin::Typed)
            .then(|| alias_prefix(text))
            .flatten()
            .and_then(|(alias, rest)| {
                if alias.is_empty() {
                    return None;
                }
                self.engines
                    .iter()
                    .find(|engine| engine.alias.eq_ignore_ascii_case(alias))
                    .map(|engine| (engine, rest))
            });
        let (engine, text) = alias.map_or((engine, text), |(engine, rest)| (Some(engine), rest));
        if text.is_empty() {
            return SearchOutcome::Empty;
        }
        let engine = engine.unwrap_or_else(|| {
            self.engines
                .iter()
                .find(|e| e.id == self.settings.preferences.default_engine)
                .expect("default engine is validated")
        });
        let url = search_url(&engine.url, text);
        let browser = if system {
            None
        } else {
            self.settings.preferences.browser.as_deref()
        };
        if let Err(error) = self.browser.open(&url, browser) {
            return SearchOutcome::Failed { query, error };
        }
        self.last_search = Some(Query {
            text: text.into(),
            engine: Some(engine.id.clone()),
            origin: Origin::History,
            source: query.source.clone(),
        });
        let history_error = if self.settings.preferences.record_history {
            let timestamp_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            self.history
                .append(Record {
                    id: 0,
                    timestamp_ms,
                    query: text.into(),
                    engine_id: engine.id.clone(),
                    engine_name: engine.name.clone(),
                    origin: query.origin,
                    source: query.source,
                })
                .err()
        } else {
            None
        };
        SearchOutcome::Opened { history_error }
    }
}

impl Record {
    pub fn repeat(self) -> Query {
        Query {
            text: self.query,
            engine: Some(self.engine_id),
            origin: Origin::History,
            source: self.source,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    Main,
    Dedicated,
}

pub enum SelectionAction {
    Ignore,
    ShowSearch,
    Preview(Query),
    Search(Query),
}

pub struct SelectionRequest {
    generation: u64,
    foreground: usize,
    deadline: Instant,
    mode: SelectionMode,
}

impl SelectionRequest {
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
}

#[derive(Default)]
pub struct SelectionRequests(u64);

impl SelectionRequests {
    pub fn begin(
        &mut self,
        foreground: usize,
        mode: SelectionMode,
        copy_allowed: bool,
        now: Instant,
    ) -> SelectionRequest {
        self.cancel();
        let budget = if copy_allowed { 850 } else { 350 };
        SelectionRequest {
            generation: self.0,
            foreground,
            deadline: now + Duration::from_millis(budget),
            mode,
        }
    }

    pub fn cancel(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }

    pub fn finish(
        &mut self,
        request: SelectionRequest,
        foreground: usize,
        now: Instant,
        text: &str,
        source: String,
        settings: &Settings,
    ) -> SelectionAction {
        if request.generation != self.0 {
            return SelectionAction::Ignore;
        }
        self.cancel();
        if foreground != request.foreground {
            return SelectionAction::Ignore;
        }
        let binding = match request.mode {
            SelectionMode::Main => None,
            SelectionMode::Dedicated => settings.preferences.selection.as_ref(),
        };
        if request.mode == SelectionMode::Dedicated && binding.is_none() {
            return SelectionAction::Ignore;
        }
        let text = if now >= request.deadline { "" } else { text };
        if trim(text).is_empty() && binding.is_none() {
            return SelectionAction::ShowSearch;
        }
        let text = if binding.is_some_and(|binding| binding.preview) {
            preview_text(text)
        } else {
            limit_text(text)
        };
        let query = Query {
            text,
            engine: binding.map(|b| b.engine.clone()),
            origin: Origin::Selection,
            source,
        };
        if query.text.is_empty() || binding.is_some_and(|binding| binding.preview) {
            SelectionAction::Preview(query)
        } else {
            SelectionAction::Search(query)
        }
    }
}

pub fn limit_text(text: &str) -> String {
    let mut remaining = 2048usize;
    text.chars()
        .take_while(|ch| {
            let Some(next) = remaining.checked_sub(ch.len_utf16()) else {
                return false;
            };
            remaining = next;
            true
        })
        .collect()
}

pub fn clipboard_query(text: &str, source: String) -> Query {
    Query {
        text: limit_text(text),
        engine: None,
        origin: Origin::Clipboard,
        source,
    }
}

fn preview_text(text: &str) -> String {
    limit_text(
        &text
            .split([' ', '\r', '\n', '\t'])
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    )
    .trim_end_matches(' ')
    .into()
}
