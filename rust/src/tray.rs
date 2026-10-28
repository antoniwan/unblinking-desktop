//! The tray icon and its menu, the Rust side of buildMenu() in main.js.
//! The same menu also pops up when you right-click the eye.

use std::error::Error;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::window::Window;

use crate::prefs::Prefs;

/// What the user asked for. The app decides what each one means; this
/// module only turns clicks into these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    ToggleShown,
    Blind(bool),
    Whispers(bool),
    Voice(bool),
    ClickThrough(bool),
    /// Take where you sit now as dead centre.
    LookAtMe,
    /// Open or close Blinki's Senses.
    Senses,
    /// Open or close "Where's my camera?".
    WhereCamera,
    StartWithComputer(bool),
    Quit,
}

pub struct Tray {
    // Kept so the icon stays in the tray; dropping it removes the icon.
    icon: TrayIcon,
    menu: Menu,
    status: MenuItem,
    shown: MenuItem,
    blind: CheckMenuItem,
    whispers: CheckMenuItem,
    voice: CheckMenuItem,
    click_through: CheckMenuItem,
    look_at_me: MenuItem,
    senses: MenuItem,
    where_camera: MenuItem,
    autostart: CheckMenuItem,
    quit: MenuItem,
}

impl Tray {
    pub fn new(prefs: &Prefs) -> Result<Tray, Box<dyn Error>> {
        let status = MenuItem::new("", false, None);
        let shown = MenuItem::new("Hide the eye", true, None);
        let blind = CheckMenuItem::new("Blind it (camera and mic off)", true, prefs.blind, None);
        let whispers = CheckMenuItem::new("Whispers", true, prefs.whispers, None);
        let voice = CheckMenuItem::new("Voice", true, prefs.voice, None);
        let click_through =
            CheckMenuItem::new("Let clicks pass through", true, prefs.click_through, None);
        let look_at_me = MenuItem::new("Look at me (sit as usual, then click)", true, None);
        let senses = MenuItem::new("Blinki's Senses  (Ctrl+Alt+B)", true, None);
        let where_camera = MenuItem::new("Where's my camera?", true, None);
        let autostart = CheckMenuItem::new(
            "Start with the computer",
            true,
            crate::session::starts_with_computer(),
            None,
        );
        let quit = MenuItem::new("Banish (quit)", true, None);

        let menu = Menu::new();
        menu.append(&status)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&shown)?;
        menu.append(&blind)?;
        menu.append(&whispers)?;
        menu.append(&voice)?;
        menu.append(&click_through)?;
        menu.append(&autostart)?;
        menu.append(&where_camera)?;
        menu.append(&look_at_me)?;
        menu.append(&senses)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit)?;

        let icon = TrayIconBuilder::new()
            .with_icon(tray_picture()?)
            // `Box::new(menu.clone())`: the tray gets its own handle on the
            // same menu, and we keep ours for the right-click popup.
            .with_menu(Box::new(menu.clone()))
            // Left click shows or hides the eye; right click opens the menu.
            .with_menu_on_left_click(false)
            .build()?;

        let tray = Tray {
            icon,
            menu,
            status,
            shown,
            blind,
            whispers,
            voice,
            click_through,
            look_at_me,
            senses,
            where_camera,
            autostart,
            quit,
        };
        tray.refresh(prefs, true);
        Ok(tray)
    }

    /// Everything clicked since the last call. Menu and tray clicks arrive
    /// on channels (like the camera's), so this never waits.
    pub fn poll(&self) -> Vec<Action> {
        let mut actions = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id();
            // A CheckMenuItem flips its own tick when clicked; we read it.
            let action = if id == self.shown.id() {
                Action::ToggleShown
            } else if id == self.blind.id() {
                Action::Blind(self.blind.is_checked())
            } else if id == self.whispers.id() {
                Action::Whispers(self.whispers.is_checked())
            } else if id == self.voice.id() {
                Action::Voice(self.voice.is_checked())
            } else if id == self.click_through.id() {
                Action::ClickThrough(self.click_through.is_checked())
            } else if id == self.autostart.id() {
                Action::StartWithComputer(self.autostart.is_checked())
            } else if id == self.where_camera.id() {
                Action::WhereCamera
            } else if id == self.senses.id() {
                Action::Senses
            } else if id == self.look_at_me.id() {
                Action::LookAtMe
            } else if id == self.quit.id() {
                Action::Quit
            } else {
                continue;
            };
            actions.push(action);
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                actions.push(Action::ToggleShown);
            }
        }
        actions
    }

    /// Brings labels, ticks and the tooltip in line with the prefs.
    pub fn refresh(&self, prefs: &Prefs, visible: bool) {
        let watching = if prefs.blind {
            "Unblinking (blinded)"
        } else {
            "Unblinking is watching"
        };
        self.status.set_text(watching);
        self.shown.set_text(if visible {
            "Hide the eye"
        } else {
            "Show the eye"
        });
        self.blind.set_checked(prefs.blind);
        self.whispers.set_checked(prefs.whispers);
        self.voice.set_checked(prefs.voice);
        self.click_through.set_checked(prefs.click_through);
        let tip = if prefs.blind {
            "Unblinking: blinded"
        } else {
            "Unblinking: watching"
        };
        let _ = self.icon.set_tooltip(Some(tip));
    }

    /// Opens the menu at the cursor, over the eye's window.
    pub fn popup(&self, window: &Window) {
        #[cfg(windows)]
        {
            use tray_icon::menu::ContextMenu;
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = window.window_handle()
                && let RawWindowHandle::Win32(h) = handle.as_raw()
            {
                // Safe as long as the HWND is a live window, which it is:
                // we're borrowing the winit Window that owns it.
                unsafe { self.menu.show_context_menu_for_hwnd(h.hwnd.get(), None) };
            }
        }
        #[cfg(not(windows))]
        let _ = window; // Mac: the menu bar icon is the way in, as in main.js.
    }
}

/// A small eye for the tray, drawn in code so there's no icon file to ship:
/// an orange disc with a dark slit pupil, soft at the edges.
fn tray_picture() -> Result<Icon, Box<dyn Error>> {
    const N: u32 = 32;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (u, v) = (
                (x as f32 + 0.5) / N as f32 * 2.0 - 1.0,
                (y as f32 + 0.5) / N as f32 * 2.0 - 1.0,
            );
            let r = (u * u + v * v).sqrt();
            let disc = ((0.95 - r) * N as f32 * 0.5).clamp(0.0, 1.0);
            // The slit: narrow in the middle, closing to points top and bottom.
            let half = 0.16 * (1.0 - v * v).max(0.0).sqrt();
            let slit = ((u.abs() - half) * N as f32 * 0.5).clamp(0.0, 1.0);
            let hot = 1.0 - r * 0.6;
            let (cr, cg, cb) = (255.0 * hot.min(1.0), 140.0 * hot * hot, 30.0 * hot);
            rgba.extend_from_slice(&[
                (cr * slit) as u8,
                (cg * slit) as u8,
                (cb * slit) as u8,
                (disc * 255.0) as u8,
            ]);
        }
    }
    Ok(Icon::from_rgba(rgba, N, N)?)
}
