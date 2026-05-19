use anyhow::{anyhow, Result};
use winvd::{create_desktop, get_desktop_count, move_window_to_desktop, switch_desktop};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::events::HotkeyAction;

pub fn execute(action: HotkeyAction) -> Result<()> {
    match action {
        HotkeyAction::SwitchToDesktop(number) => switch_to_number(number),
        HotkeyAction::MoveFocusedAndSwitch(number) => move_focused_and_switch(number),
    }
}

fn switch_to_number(number: u32) -> Result<()> {
    let target_index = match number.checked_sub(1) {
        Some(index) => index,
        None => return Ok(()),
    };

    ensure_desktop_exists(target_index)?;

    switch_desktop(target_index as i32)
        .map_err(|err| anyhow!("failed switching to desktop {number}: {err:?}"))?;
    tracing::info!("switched to desktop {}", number);
    Ok(())
}

fn move_focused_and_switch(number: u32) -> Result<()> {
    let target_index = match number.checked_sub(1) {
        Some(index) => index,
        None => return Ok(()),
    };

    ensure_desktop_exists(target_index)?;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        tracing::warn!("no focused window available for move action");
        return Ok(());
    }

    move_window_to_desktop(target_index as i32, &hwnd)
        .map_err(|err| anyhow!("failed moving focused window to desktop {number}: {err:?}"))?;
    switch_desktop(target_index as i32)
        .map_err(|err| anyhow!("failed switching to desktop {number}: {err:?}"))?;
    tracing::info!("moved focused window and switched to desktop {}", number);
    Ok(())
}

fn ensure_desktop_exists(target_index: u32) -> Result<()> {
    loop {
        let count =
            get_desktop_count().map_err(|err| anyhow!("failed reading desktop count: {err:?}"))?;
        if target_index < count {
            return Ok(());
        }

        create_desktop().map_err(|err| anyhow!("failed creating desktop: {err:?}"))?;
        tracing::info!("created desktop {}", count + 1);
    }
}
