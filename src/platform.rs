mod hotkeys;
mod selection;

pub use hotkeys::{Hotkey, HotkeySet, KeyBindings};
pub use selection::{Foreground, SelectionCopy, SelectionError, clipboard_text};

use crate::application::Browser;
use crate::engine::is_browser_url;
use crate::storage::{StorageError, read_json, write_json};
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE, LocalFree,
};
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::{
    ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, AssocQueryStringW, CommandLineToArgvW, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain([0]).collect()
}

pub struct Instance(HANDLE);

impl Instance {
    pub fn acquire() -> io::Result<Option<Self>> {
        let name = wide("Local\\YiSou.Rust.Resident");
        let handle = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let instance = Self(handle);
        Ok((!already_exists).then_some(instance))
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub fn data_directory() -> io::Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA"))?;
    Ok(PathBuf::from(local).join("YiSouRust"))
}

pub fn settings_path(
    explicit: Option<PathBuf>,
    data: &Path,
    executable: &Path,
) -> Result<PathBuf, StorageError> {
    if let Some(path) = explicit {
        return Ok(std::path::absolute(path)?);
    }
    match read_json::<PathBuf>(&data.join("config-path.json")) {
        Ok(path) if path.is_absolute() => return Ok(path),
        Ok(_) => {
            return Err(
                io::Error::new(io::ErrorKind::InvalidData, "invalid remembered path").into(),
            );
        }
        Err(StorageError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let portable = executable.with_file_name("settings.json");
    Ok(if portable.try_exists()? {
        portable
    } else {
        data.join("settings.json")
    })
}

pub fn remember_settings_path(data: &Path, path: &Path) -> Result<(), StorageError> {
    write_json(&data.join("config-path.json"), &std::path::absolute(path)?)
}

struct Key(HKEY);

impl Key {
    fn open(root: HKEY, path: &str, access: REG_SAM_FLAGS) -> io::Result<Self> {
        let mut key = null_mut();
        let status = unsafe { RegOpenKeyExW(root, wide(path).as_ptr(), 0, access, &mut key) };
        if status == 0 {
            Ok(Self(key))
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }

    fn string(&self, path: &str) -> Option<OsString> {
        let path = wide(path);
        let mut bytes = 0;
        let types = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
        let status = unsafe {
            RegGetValueW(
                self.0,
                path.as_ptr(),
                null(),
                types,
                null_mut(),
                null_mut(),
                &mut bytes,
            )
        };
        if status != 0 || !(2..=65536).contains(&bytes) || bytes % 2 != 0 {
            return None;
        }
        let mut value = vec![0u16; bytes as usize / 2];
        let status = unsafe {
            RegGetValueW(
                self.0,
                path.as_ptr(),
                null(),
                types,
                null_mut(),
                value.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        (status == 0).then(|| {
            OsString::from_wide(
                &value[..value
                    .iter()
                    .position(|&unit| unit == 0)
                    .unwrap_or(value.len())],
            )
        })
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledBrowser {
    pub name: String,
    pub path: PathBuf,
}

fn valid_browser(path: &Path) -> bool {
    path.is_absolute()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        && path.is_file()
}

fn browser_path(command: OsString) -> Option<PathBuf> {
    let direct = PathBuf::from(&command);
    if valid_browser(&direct) {
        return Some(direct);
    }
    let mut count = 0;
    let args = unsafe { CommandLineToArgvW(wide(command).as_ptr(), &mut count) };
    if args.is_null() {
        return None;
    }
    let result = if count > 0 {
        let program = unsafe { *args };
        let length = (0..)
            .take_while(|&offset| unsafe { *program.add(offset) != 0 })
            .count();
        let path = PathBuf::from(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(program, length)
        }));
        valid_browser(&path).then_some(path)
    } else {
        None
    };
    unsafe { LocalFree(args.cast()) };
    result
}

pub fn default_browser() -> Option<PathBuf> {
    let protocol = wide("https");
    let mut length = 0;
    unsafe {
        AssocQueryStringW(
            ASSOCF_IS_PROTOCOL,
            ASSOCSTR_EXECUTABLE,
            protocol.as_ptr(),
            null(),
            null_mut(),
            &mut length,
        )
    };
    if !(2..=32768).contains(&length) {
        return None;
    }
    let mut value = vec![0; length as usize];
    let status = unsafe {
        AssocQueryStringW(
            ASSOCF_IS_PROTOCOL,
            ASSOCSTR_EXECUTABLE,
            protocol.as_ptr(),
            null(),
            value.as_mut_ptr(),
            &mut length,
        )
    };
    if status < 0 {
        return None;
    }
    browser_path(OsString::from_wide(
        &value[..value.iter().position(|&unit| unit == 0)?],
    ))
}

pub fn installed_browsers() -> Vec<InstalledBrowser> {
    let mut browsers = Vec::<InstalledBrowser>::new();
    let mut add = |path: PathBuf, label: Option<OsString>| {
        if browsers.iter().any(|browser| {
            browser
                .path
                .as_os_str()
                .eq_ignore_ascii_case(path.as_os_str())
        }) {
            return;
        }
        let name = label
            .filter(|value| !value.is_empty() && !value.to_string_lossy().starts_with('@'))
            .or_else(|| path.file_stem().map(OsStr::to_os_string))
            .unwrap_or_default();
        browsers.push(InstalledBrowser {
            name: name.to_string_lossy().into_owned(),
            path,
        });
    };
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let Ok(key) = Key::open(
                hive,
                "Software\\Clients\\StartMenuInternet",
                KEY_READ | view,
            ) else {
                continue;
            };
            for index in 0.. {
                let mut name = [0u16; 256];
                let mut size = name.len() as u32;
                let status = unsafe {
                    RegEnumKeyExW(
                        key.0,
                        index,
                        name.as_mut_ptr(),
                        &mut size,
                        null(),
                        null_mut(),
                        null_mut(),
                        null_mut(),
                    )
                };
                if status != 0 {
                    break;
                }
                let name = String::from_utf16_lossy(&name[..size as usize]);
                if let Some(path) = key
                    .string(&format!("{name}\\shell\\open\\command"))
                    .and_then(browser_path)
                {
                    add(path, key.string(&name));
                }
            }
        }
    }
    if let Some(path) = default_browser() {
        add(path, None);
    }
    browsers.sort_by_cached_key(|browser| browser.name.to_lowercase());
    browsers
}

pub struct WindowsBrowser;

impl Browser for WindowsBrowser {
    fn open(&mut self, url: &str, executable: Option<&Path>) -> io::Result<()> {
        if !is_browser_url(url) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid URL"));
        }
        if let Some(path) = executable {
            if !valid_browser(path) {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "browser executable not found",
                ));
            }
            let mut command = Command::new(path);
            if let Some(parent) = path.parent() {
                command.current_dir(parent);
            }
            command
                .arg(url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            return Ok(());
        }
        let status = unsafe {
            ShellExecuteW(
                null_mut(),
                wide("open").as_ptr(),
                wide(url).as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if status > 32 {
            Ok(())
        } else {
            Err(io::Error::other(format!("ShellExecuteW returned {status}")))
        }
    }
}

fn quote_arg(argument: &OsStr) -> Vec<u16> {
    let mut output = vec![b'"' as u16];
    let mut slashes = 0;
    for ch in argument.encode_wide() {
        if ch == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        if ch == b'"' as u16 {
            slashes = slashes * 2 + 1;
        }
        output.extend(std::iter::repeat_n(b'\\' as u16, slashes));
        output.push(ch);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    output
}

pub fn set_startup(executable: &Path, config: Option<&Path>, enabled: bool) -> io::Result<()> {
    let key = Key::open(
        HKEY_CURRENT_USER,
        "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        KEY_SET_VALUE,
    )?;
    let name = wide("YiSouRust");
    let status = if enabled {
        let mut command = quote_arg(executable.as_os_str());
        command.extend(" --startup".encode_utf16());
        if let Some(config) = config {
            command.extend(" --config ".encode_utf16());
            command.extend(quote_arg(config.as_os_str()));
        }
        command.push(0);
        if command.len() > 260 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "startup command exceeds Run key limit",
            ));
        }
        unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * 2) as u32,
            )
        }
    } else {
        let status = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            0
        } else {
            status
        }
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}
