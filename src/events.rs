#[derive(Debug, Clone, Copy)]
pub enum HotkeyAction {
    SwitchToDesktop(u32),
    MoveFocusedAndSwitch(u32),
}

#[derive(Debug, Clone, Copy)]
pub enum AppEvent {
    Hotkey(HotkeyAction),
    ToggleHooks,
    ToggleStartup,
    Exit,
}
