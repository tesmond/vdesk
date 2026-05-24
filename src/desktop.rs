use anyhow::{Result, anyhow};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
        SetForegroundWindow,
    },
};
use winvd::{
    create_desktop, get_current_desktop, get_desktop_count, is_window_on_desktop,
    move_window_to_desktop, switch_desktop,
};

use crate::events::HotkeyAction;

static FOCUSED_WINDOWS: OnceLock<Mutex<HashMap<u32, FocusedWindow>>> = OnceLock::new();

#[derive(Clone, Copy)]
struct FocusedWindow {
    hwnd: isize,
    process_id: u32,
}

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
    remember_current_focus()?;

    switch_desktop(target_index as i32)
        .map_err(|err| anyhow!("failed switching to desktop {number}: {err:?}"))?;
    refocus_desktop(target_index);

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
    if !is_focusable_window(hwnd) {
        tracing::warn!("no focused window available for move action");
        return Ok(());
    }

    remember_current_focus()?;

    move_window_to_desktop(target_index as i32, &hwnd)
        .map_err(|err| anyhow!("failed moving focused window to desktop {number}: {err:?}"))?;
    remember_focus_for_desktop(target_index, hwnd);

    switch_desktop(target_index as i32)
        .map_err(|err| anyhow!("failed switching to desktop {number}: {err:?}"))?;
    refocus_desktop(target_index);

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

fn remember_current_focus() -> Result<()> {
    let current_index = get_current_desktop()
        .and_then(|desktop| desktop.get_index())
        .map_err(|err| anyhow!("failed reading current desktop: {err:?}"))?;

    let hwnd = unsafe { GetForegroundWindow() };
    if is_focusable_window(hwnd) {
        remember_focus_for_desktop(current_index, hwnd);
    }

    Ok(())
}

fn remember_focus_for_desktop(desktop_index: u32, hwnd: HWND) {
    let Some(process_id) = window_process_id(hwnd) else {
        return;
    };

    let focused = FOCUSED_WINDOWS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut guard) = focused.lock() else {
        tracing::warn!("failed locking focused window memory");
        return;
    };

    guard.insert(
        desktop_index,
        FocusedWindow {
            hwnd: hwnd.0 as isize,
            process_id,
        },
    );
}

fn refocus_desktop(desktop_index: u32) {
    let Some(remembered) = remembered_focus_for_desktop(desktop_index) else {
        return;
    };

    thread::sleep(Duration::from_millis(50));

    let hwnd = remembered.hwnd();
    if !is_remembered_window_alive(remembered) {
        forget_focus_for_desktop(desktop_index);
        return;
    }

    match is_window_on_desktop(desktop_index as i32, hwnd) {
        Ok(true) => {}
        Ok(false) => {
            forget_focus_for_desktop(desktop_index);
            return;
        }
        Err(err) => {
            tracing::warn!("failed checking remembered window desktop: {err:?}");
            forget_focus_for_desktop(desktop_index);
            return;
        }
    }

    if !unsafe { SetForegroundWindow(hwnd) }.as_bool() {
        tracing::warn!(
            "failed refocusing remembered window for desktop {}",
            desktop_index + 1
        );
    }
}

fn remembered_focus_for_desktop(desktop_index: u32) -> Option<FocusedWindow> {
    let focused = FOCUSED_WINDOWS.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = focused.lock().ok()?;
    guard.get(&desktop_index).copied()
}

fn forget_focus_for_desktop(desktop_index: u32) {
    let focused = FOCUSED_WINDOWS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut guard) = focused.lock() else {
        return;
    };

    guard.remove(&desktop_index);
}

fn is_focusable_window(hwnd: HWND) -> bool {
    !hwnd.0.is_null()
        && unsafe { IsWindow(hwnd) }.as_bool()
        && unsafe { IsWindowVisible(hwnd) }.as_bool()
        && window_process_id(hwnd).is_some()
}

fn is_remembered_window_alive(window: FocusedWindow) -> bool {
    let hwnd = window.hwnd();
    is_focusable_window(hwnd) && window_process_id(hwnd) == Some(window.process_id)
}

fn window_process_id(hwnd: HWND) -> Option<u32> {
    if hwnd.0.is_null() {
        return None;
    }

    let mut process_id = 0;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if thread_id == 0 || process_id == 0 {
        return None;
    }

    Some(process_id)
}

impl FocusedWindow {
    fn hwnd(self) -> HWND {
        HWND(self.hwnd as *mut std::ffi::c_void)
    }
}
