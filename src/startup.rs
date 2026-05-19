use anyhow::{Context, Result};
use std::env;
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE},
    RegKey,
};

const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE_NAME: &str = "vdesk";

pub fn set_enabled(enabled: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey_with_flags(RUN_KEY_PATH, KEY_READ | KEY_SET_VALUE)
        .context("failed opening HKCU Run key")?;

    if enabled {
        let exe = env::current_exe().context("failed resolving current executable")?;
        key.set_value(RUN_VALUE_NAME, &exe.to_string_lossy().to_string())
            .context("failed setting startup registry value")?;
    } else {
        let _ = key.delete_value(RUN_VALUE_NAME);
    }

    Ok(())
}
