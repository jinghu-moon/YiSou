pub mod application;
pub mod engine;
pub mod icons;
pub mod launch;
#[cfg(all(windows, feature = "ocr"))]
pub mod ocr;
pub mod settings;
pub mod shortcut;
pub mod storage;

#[cfg(windows)]
pub mod platform;
