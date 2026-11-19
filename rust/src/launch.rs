use crate::settings::Preferences;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LaunchMode {
    #[default]
    Manual,
    Startup,
    Settings,
    Configuration,
    IconPreview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowAction {
    StayInTray,
    ShowSearch,
    ShowSettings,
    ShowConfiguration,
    ShowIconPreview,
    Exit,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Launch {
    pub mode: LaunchMode,
    pub config: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("--config requires a path")]
    MissingPath,
    #[error("unknown argument {0:?}")]
    UnknownArgument(OsString),
}

impl Launch {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, LaunchError> {
        let mut args = args.into_iter();
        let mut launch = Self::default();
        let mut preview = false;
        while let Some(argument) = args.next() {
            match argument.to_str() {
                Some("--startup") => launch.mode = LaunchMode::Startup,
                Some("--settings") => launch.mode = LaunchMode::Settings,
                Some("--configuration") => launch.mode = LaunchMode::Configuration,
                Some("--icon-preview") => preview = true,
                Some("--config") => {
                    let path = args
                        .next()
                        .filter(|path| !path.is_empty())
                        .ok_or(LaunchError::MissingPath)?;
                    launch.config = Some(path.into());
                }
                _ => return Err(LaunchError::UnknownArgument(argument)),
            }
        }
        if preview {
            launch.mode = LaunchMode::IconPreview;
        }
        Ok(launch)
    }

    pub fn action(&self, preferences: &Preferences, existing_instance: bool) -> WindowAction {
        match self.mode {
            LaunchMode::Startup => WindowAction::StayInTray,
            LaunchMode::Manual if preferences.show_on_manual_launch || existing_instance => {
                WindowAction::ShowSearch
            }
            LaunchMode::Manual => WindowAction::StayInTray,
            LaunchMode::Settings => WindowAction::ShowSettings,
            LaunchMode::Configuration => WindowAction::ShowConfiguration,
            LaunchMode::IconPreview => WindowAction::ShowIconPreview,
        }
    }
}

pub fn close_search(preferences: &Preferences) -> WindowAction {
    if preferences.close_to_tray {
        WindowAction::StayInTray
    } else {
        WindowAction::Exit
    }
}
