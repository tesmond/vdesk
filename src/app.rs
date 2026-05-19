use anyhow::{anyhow, Context, Result};
use std::sync::mpsc;
use tray_item::{IconSource, TrayItem};
use windows::{
    Win32::{
        Foundation::BOOL,
        Graphics::Gdi::{
            CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO,
            BITMAPV5HEADER, BI_BITFIELDS, DIB_RGB_COLORS,
        },
        UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO},
    },
};

use crate::{config, desktop, events::AppEvent, hotkeys::KeyboardHook, startup};

pub fn run() -> Result<()> {
    let (event_tx, event_rx) = mpsc::channel::<AppEvent>();

    let mut app_config = config::load().unwrap_or_default();
    startup::set_enabled(app_config.autostart_enabled)?;

    let mut tray = build_tray(event_tx.clone())?;
    tray.add_label("vdesk active")?;

    let mut hook = if app_config.hooks_enabled {
        Some(KeyboardHook::start(event_tx.clone())?)
    } else {
        None
    };

    loop {
        let event = event_rx.recv().context("event channel closed")?;

        match event {
            AppEvent::Hotkey(action) => {
                if app_config.hooks_enabled {
                    if let Err(err) = desktop::execute(action) {
                        tracing::error!("desktop action failed: {err:#}");
                    }
                }
            }
            AppEvent::ToggleHooks => {
                app_config.hooks_enabled = !app_config.hooks_enabled;

                if app_config.hooks_enabled {
                    hook = Some(KeyboardHook::start(event_tx.clone())?);
                    tracing::info!("hotkey hooks enabled");
                } else {
                    hook = None;
                    tracing::info!("hotkey hooks disabled");
                }

                config::save(&app_config)?;
            }
            AppEvent::ToggleStartup => {
                app_config.autostart_enabled = !app_config.autostart_enabled;
                startup::set_enabled(app_config.autostart_enabled)?;
                config::save(&app_config)?;
                tracing::info!("startup enabled: {}", app_config.autostart_enabled);
            }
            AppEvent::Exit => break,
        }
    }

    drop(hook);
    drop(tray);

    Ok(())
}

fn build_tray(event_tx: mpsc::Sender<AppEvent>) -> Result<TrayItem> {
    let icon = load_png_icon(include_bytes!("vdesk.png"))?;
    let mut tray = TrayItem::new("vdesk", IconSource::RawIcon(icon))?;

    {
        let tx = event_tx.clone();
        tray.add_menu_item("Toggle hooks", move || {
            let _ = tx.send(AppEvent::ToggleHooks);
        })?;
    }

    {
        let tx = event_tx.clone();
        tray.add_menu_item("Toggle startup", move || {
            let _ = tx.send(AppEvent::ToggleStartup);
        })?;
    }

    {
        let tx = event_tx;
        tray.add_menu_item("Exit", move || {
            let _ = tx.send(AppEvent::Exit);
        })?;
    }

    Ok(tray)
}

fn load_png_icon(bytes: &[u8]) -> Result<isize> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| anyhow!("failed to decode icon PNG: {e}"))?
        .into_rgba8();

    let (width, height) = img.dimensions();

    // Convert RGBA → BGRA (Win32 DIB byte order)
    let mut bgra: Vec<u8> = Vec::with_capacity((width * height * 4) as usize);
    for px in img.pixels() {
        bgra.push(px[2]); // B
        bgra.push(px[1]); // G
        bgra.push(px[0]); // R
        bgra.push(px[3]); // A
    }

    // Build a BITMAPV5HEADER so the alpha channel is preserved correctly
    let mut bmi: BITMAPV5HEADER = unsafe { std::mem::zeroed() };
    bmi.bV5Size = std::mem::size_of::<BITMAPV5HEADER>() as u32;
    bmi.bV5Width = width as i32;
    bmi.bV5Height = -(height as i32); // negative = top-down scan lines
    bmi.bV5Planes = 1;
    bmi.bV5BitCount = 32;
    bmi.bV5Compression = BI_BITFIELDS;
    bmi.bV5RedMask = 0x00FF_0000;
    bmi.bV5GreenMask = 0x0000_FF00;
    bmi.bV5BlueMask = 0x0000_00FF;
    bmi.bV5AlphaMask = 0xFF00_0000;

    let hdc = unsafe { GetDC(None) };

    let mut bits = std::ptr::null_mut();
    let hbm_color = unsafe {
        CreateDIBSection(
            hdc,
            &bmi as *const BITMAPV5HEADER as *const BITMAPINFO,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )
    }
    .map_err(|e| anyhow!("CreateDIBSection failed: {e}"))?;

    unsafe { std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len()) };

    // 1-bit mask bitmap: all-zero means the alpha channel fully governs visibility
    let hbm_mask = unsafe { CreateBitmap(width as i32, height as i32, 1, 1, None) };

    let icon_info = ICONINFO {
        fIcon: BOOL(1),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };

    let hicon =
        unsafe { CreateIconIndirect(&icon_info) }.map_err(|e| anyhow!("CreateIconIndirect failed: {e}"))?;

    // Release temporary GDI objects (the HICON holds its own copies)
    unsafe {
        ReleaseDC(None, hdc);
        let _ = DeleteObject(hbm_color);
        let _ = DeleteObject(hbm_mask);
    }

    Ok(hicon.0 as isize)
}
