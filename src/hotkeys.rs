use anyhow::{Context, Result};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Mutex, OnceLock,
    },
    thread,
};
use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LWIN, VK_RWIN, VK_SHIFT},
        WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
            HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
            WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    },
};

use crate::events::{AppEvent, HotkeyAction};

static EVENT_TX: OnceLock<Sender<AppEvent>> = OnceLock::new();
static CONSUMED_KEYS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
static WIN_KEY_DOWN: AtomicBool = AtomicBool::new(false);
static HOTKEY_TRIGGERED: AtomicBool = AtomicBool::new(false);

pub struct KeyboardHook {
    thread_id: u32,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl KeyboardHook {
    pub fn start(event_tx: Sender<AppEvent>) -> Result<Self> {
        let _ = EVENT_TX.set(event_tx);
        let _ = CONSUMED_KEYS.set(Mutex::new(HashSet::new()));

        let (id_tx, id_rx) = std::sync::mpsc::channel::<u32>();
        let join_handle = thread::Builder::new()
            .name("vdesk-hook".to_string())
            .spawn(move || {
                let thread_id = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
                let _ = id_tx.send(thread_id);

                let module = unsafe { GetModuleHandleW(None) }
                    .map(HINSTANCE::from)
                    .unwrap_or_default();

                let hook = match unsafe {
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), module, 0)
                } {
                    Ok(hook) => hook,
                    Err(err) => {
                        tracing::error!("failed to install low-level keyboard hook: {err}");
                        return;
                    }
                };

                let mut msg = MSG::default();
                loop {
                    let status = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                    if status.0 <= 0 {
                        break;
                    }
                }

                let _ = unsafe { UnhookWindowsHookEx(hook) };
            })
            .context("failed spawning keyboard hook thread")?;

        let thread_id = id_rx.recv().context("failed reading hook thread id")?;

        Ok(Self {
            thread_id,
            join_handle: Some(join_handle),
        })
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }

        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        let vk = kb.vkCode;

        let is_keydown = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let is_keyup = message == WM_KEYUP || message == WM_SYSKEYUP;
        let is_win = is_win_key(vk);

        // Track Win key state explicitly
        if is_keydown && is_win {
            WIN_KEY_DOWN.store(true, Ordering::SeqCst);
        }

        if is_keyup && is_win {
            WIN_KEY_DOWN.store(false, Ordering::SeqCst);
            // If a hotkey was triggered, suppress the Win key release
            if HOTKEY_TRIGGERED.swap(false, Ordering::SeqCst) {
                return LRESULT(1);
            }
        }

        if is_keydown && is_win {
            // Suppress Win key press - we'll decide whether to let it through on release
            return LRESULT(1);
        }

        if is_keydown {
            if let Some(action) = decode_hotkey(vk) {
                HOTKEY_TRIGGERED.store(true, Ordering::SeqCst);
                if mark_key_consumed(vk) {
                    if let Some(tx) = EVENT_TX.get() {
                        let _ = tx.send(AppEvent::Hotkey(action));
                    }
                }
                return LRESULT(1);
            }
        }

        if is_keyup && unmark_key_consumed(vk) {
            return LRESULT(1);
        }
    }

    unsafe { CallNextHookEx(HHOOK(std::ptr::null_mut()), code, wparam, lparam) }
}

fn decode_hotkey(vk_code: u32) -> Option<HotkeyAction> {
    if !win_pressed() {
        return None;
    }

    let desktop_number = digit_from_vk(vk_code)?;
    if shift_pressed() {
        Some(HotkeyAction::MoveFocusedAndSwitch(desktop_number))
    } else {
        Some(HotkeyAction::SwitchToDesktop(desktop_number))
    }
}

fn digit_from_vk(vk_code: u32) -> Option<u32> {
    match vk_code {
        0x31 => Some(1),
        0x32 => Some(2),
        0x33 => Some(3),
        0x34 => Some(4),
        0x35 => Some(5),
        0x36 => Some(6),
        0x37 => Some(7),
        0x38 => Some(8),
        0x39 => Some(9),
        _ => None,
    }
}

fn win_pressed() -> bool {
    WIN_KEY_DOWN.load(Ordering::SeqCst)
}

fn shift_pressed() -> bool {
    key_down(VK_SHIFT.0 as i32)
}

fn key_down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16) & 0x8000 != 0 }
}

fn is_win_key(vk_code: u32) -> bool {
    vk_code == VK_LWIN.0 as u32 || vk_code == VK_RWIN.0 as u32
}

fn mark_key_consumed(vk_code: u32) -> bool {
    let Some(consumed) = CONSUMED_KEYS.get() else {
        return true;
    };

    let mut guard = match consumed.lock() {
        Ok(guard) => guard,
        Err(_) => return true,
    };

    guard.insert(vk_code)
}

fn unmark_key_consumed(vk_code: u32) -> bool {
    let Some(consumed) = CONSUMED_KEYS.get() else {
        return false;
    };

    let mut guard = match consumed.lock() {
        Ok(guard) => guard,
        Err(_) => return false,
    };

    guard.remove(&vk_code)
}
