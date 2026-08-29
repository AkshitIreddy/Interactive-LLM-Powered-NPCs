use std::sync::atomic::{AtomicBool, Ordering};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Runtime, Window, WindowEvent};

const TRAY_ID: &str = "response-console-tray";
const MENU_SHOW: &str = "show-console";
const MENU_QUIT: &str = "quit-application";

#[derive(Debug, Default)]
pub struct LifecycleState {
    exit_requested: AtomicBool,
    tray_available: AtomicBool,
}

impl LifecycleState {
    fn request_exit(&self) {
        self.exit_requested.store(true, Ordering::Release);
    }

    fn should_hide_to_tray(&self) -> bool {
        self.tray_available.load(Ordering::Acquire) && !self.exit_requested.load(Ordering::Acquire)
    }

    fn set_tray_available(&self) {
        self.tray_available.store(true, Ordering::Release);
    }
}

pub fn install_tray<R: Runtime>(app: &mut App<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, MENU_SHOW, "Open Response Console", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Interactive NPCs — Response Console")
        .icon(generated_tray_icon())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_main_window(app),
            MENU_QUIT => request_application_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    app.state::<LifecycleState>().set_tray_available();
    Ok(())
}

pub fn handle_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        let lifecycle = window.state::<LifecycleState>();
        if lifecycle.should_hide_to_tray() {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

fn request_application_exit<R: Runtime>(app: &AppHandle<R>) {
    app.state::<LifecycleState>().request_exit();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        app.state::<crate::commands::AppState>()
            .shutdown_services()
            .await;
        app.exit(0);
    });
}

/// Produce a deliberately simple development-only graphite/teal glyph. Release
/// artwork is a provenance-tracked design asset and is not approximated here.
fn generated_tray_icon() -> Image<'static> {
    const SIZE: usize = 32;
    let mut rgba = vec![0_u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let offset = (y * SIZE + x) * 4;
            let rounded = (3..SIZE - 3).contains(&x) || (3..SIZE - 3).contains(&y);
            if rounded {
                rgba[offset..offset + 4].copy_from_slice(&[25, 31, 34, 255]);
            }
            let left_stem = (8..=11).contains(&x) && (7..=24).contains(&y);
            let right_stem = (20..=23).contains(&x) && (7..=24).contains(&y);
            let diagonal = (8..=23).contains(&x)
                && (7..=24).contains(&y)
                && ((x as isize - 8) - (y as isize - 7)).unsigned_abs() <= 2;
            if left_stem || right_stem || diagonal {
                rgba[offset..offset + 4].copy_from_slice(&[99, 214, 195, 255]);
            }
        }
    }
    Image::new_owned(rgba, SIZE as u32, SIZE as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_only_hides_after_tray_is_ready() {
        let state = LifecycleState::default();
        assert!(!state.should_hide_to_tray());
        state.set_tray_available();
        assert!(state.should_hide_to_tray());
        state.request_exit();
        assert!(!state.should_hide_to_tray());
    }

    #[test]
    fn generated_icon_has_expected_dimensions() {
        let icon = generated_tray_icon();
        assert_eq!(icon.width(), 32);
        assert_eq!(icon.height(), 32);
        assert_eq!(icon.rgba().len(), 32 * 32 * 4);
    }
}
