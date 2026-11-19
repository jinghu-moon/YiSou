use crate::settings::{EnginePack, Settings, SettingsError};
use serde::{Serialize, de::DeserializeOwned};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Format(#[from] serde_json::Error),
    #[error(transparent)]
    Settings(#[from] SettingsError),
}

pub trait SettingsStore {
    fn save(&mut self, settings: &Settings) -> Result<(), StorageError>;
}

pub struct JsonSettings {
    path: PathBuf,
}

impl JsonSettings {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Settings, StorageError> {
        match import_settings(&self.path) {
            Err(StorageError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                Ok(Settings::default())
            }
            other => other,
        }
    }
}

impl SettingsStore for JsonSettings {
    fn save(&mut self, settings: &Settings) -> Result<(), StorageError> {
        write_json(&self.path, settings)
    }
}

pub fn import_settings(path: &Path) -> Result<Settings, StorageError> {
    let mut settings: Settings = read_json(path)?;
    settings.normalize()?;
    Ok(settings)
}

pub fn import_engines(path: &Path) -> Result<EnginePack, StorageError> {
    read_json(path)
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, StorageError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_FILE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file is too large").into());
    }
    Ok(serde_json::from_reader(
        BufReader::new(file).take(MAX_FILE_BYTES + 1),
    )?)
}

pub(crate) fn write_json<T: Serialize>(path: &Path, data: &T) -> Result<(), StorageError> {
    let directory = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    {
        let mut writer = BufWriter::new(file.as_file_mut());
        serde_json::to_writer_pretty(&mut writer, data)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    if file.as_file().metadata()?.len() > MAX_FILE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file is too large").into());
    }
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}
