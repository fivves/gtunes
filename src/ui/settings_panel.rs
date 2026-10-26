//! The settings surface.
//!
//! Settings used to be a `GtkPopover` hanging off the toolbar button. A popover
//! is its own surface, so it is composited outside the app window: with gTunes
//! parked against the right edge of the display the menu spilled out of the
//! window and got clipped by the screen instead of being contained by the app.
//!
//! This is the same menu as an overlay child of the window, so the window itself
//! clips it, and `sync_settings_panel` clamps its rect inside the window on every
//! open and on every resize while it is up. Nothing here is allowed to position
//! itself outside the toplevel.

use super::prelude::*;

/// Panel width, logical px. The popover content asked for 280, plus its own
/// padding, so this keeps the menu the same size he is used to.
pub(crate) const SETTINGS_PANEL_WIDTH: i32 = 292;
/// Gap between the toolbar button and the panel's top edge.
const SETTINGS_PANEL_GAP: i32 = 6;
/// Closest the panel may sit to any window edge. Every clamp below keeps the
/// panel at least this far inside the window.
pub(crate) const SETTINGS_PANEL_EDGE: i32 = 10;
/// Narrowest the panel is allowed to get before the rows start wrapping.
const SETTINGS_PANEL_MIN_WIDTH: i32 = 200;
/// Slide/fade duration. Same motion language as the rest of the app.
const SETTINGS_PANEL_ANIM_MS: u32 = 180;

/// Build the scrim and the panel, and hang both off the window overlay. Called
/// once from `window::build` after the app root has been set as the overlay's
/// main child.
pub(crate) fn install_settings_panel(overlay: &gtk::Overlay, state: Rc<RefCell<UiState>>) {
    // Dim + click-catcher. Fills the window, so a click anywhere outside the
    // panel dismisses it, the way a menu is expected to behave.
    let scrim = gtk::Box::new(Orientation::Vertical, 0);
    scrim.add_css_class("settings-scrim");
    scrim.add_css_class("settings-closed");
    scrim.set_halign(Align::Fill);
    scrim.set_valign(Align::Fill);
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_visible(false);
    {
        let state = state.clone();
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, _, _, _| close_settings_panel(&state));
        scrim.add_controller(click);
    }
    overlay.add_overlay(&scrim);

    let content = build_settings_content(state.clone());
    content.add_css_class("settings-popover-menu");

    // Cap the height instead of letting a short window push the panel's last
    // rows past the bottom edge: the scroll window takes whatever room the
    // panel actually has, and only then does the menu scroll.
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .child(&content)
        .build();
    scroll.add_css_class("settings-panel-scroll");

    let panel = gtk::Box::new(Orientation::Vertical, 0);
    panel.add_css_class("settings-panel");
    panel.append(&scroll);

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let state = state.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                close_settings_panel(&state);
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
    }
    panel.add_controller(keys);

    let revealer = gtk::Revealer::new();
    revealer.add_css_class("settings-revealer");
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(SETTINGS_PANEL_ANIM_MS);
    revealer.set_child(Some(&panel));
    revealer.set_halign(Align::End);
    revealer.set_valign(Align::Start);
    revealer.set_visible(false);
    overlay.add_overlay(&revealer);

    // Tuning hook: `GTUNES_SETTINGS_OPEN=1` opens the panel shortly after
    // startup, so its containment can be screenshotted and read out of a log
    // without driving a mouse. Only active when the variable is set.
    if std::env::var_os("GTUNES_SETTINGS_OPEN").is_some() {
        let state = state.clone();
        gtk::glib::timeout_add_local_once(Duration::from_millis(900), move || {
            open_settings_panel(&state);
        });
    }

    let mut ui = state.borrow_mut();
    ui.window_overlay = Some(overlay.clone());
    ui.settings_scrim = Some(scrim);
    ui.settings_revealer = Some(revealer);
    ui.settings_panel = Some(panel);
    ui.settings_scroll = Some(scroll);
}

pub(crate) fn settings_panel_open(state: &Rc<RefCell<UiState>>) -> bool {
    state.borrow().settings_panel_open
}

/// Show the panel. Idempotent: opening an open panel only re-clamps it.
pub(crate) fn open_settings_panel(state: &Rc<RefCell<UiState>>) {
    if state.borrow().settings_panel_open {
        sync_settings_panel(state);
        return;
    }

    let (revealer, scrim, animations) = {
        let mut ui = state.borrow_mut();
        ui.settings_panel_open = true;
        // The narrow tiers move the toolbar buttons into this popover, and the
        // panel is drawn under it, so it has to go.
        if let Some(compact) = ui.compact_menu.as_ref() {
            compact.set_active(false);
        }
        (
            ui.settings_revealer.clone(),
            ui.settings_scrim.clone(),
            ui.animations_enabled,
        )
    };

    set_settings_button_active(state, true);

    if let Some(revealer) = revealer.as_ref() {
        revealer.set_transition_duration(if animations {
            SETTINGS_PANEL_ANIM_MS
        } else {
            0
        });
        revealer.set_visible(true);
        revealer.set_reveal_child(true);
    }
    if let Some(scrim) = scrim.as_ref() {
        scrim.set_visible(true);
        scrim.remove_css_class("settings-closed");
    }

    sync_settings_panel(state);
}

/// Hide the panel. Also idempotent.
pub(crate) fn close_settings_panel(state: &Rc<RefCell<UiState>>) {
    if !state.borrow().settings_panel_open {
        return;
    }

    let (revealer, animations) = {
        let mut ui = state.borrow_mut();
        ui.settings_panel_open = false;
        (ui.settings_revealer.clone(), ui.animations_enabled)
    };

    set_settings_button_active(state, false);

    if let Some(revealer) = revealer.as_ref() {
        revealer.set_transition_duration(if animations {
            SETTINGS_PANEL_ANIM_MS
        } else {
            0
        });
        revealer.set_reveal_child(false);
    }

    let timeout_ms = if animations {
        SETTINGS_PANEL_ANIM_MS as u64
    } else {
        0
    };
    {
        let state = state.clone();
        gtk::glib::timeout_add_local_once(Duration::from_millis(timeout_ms), move || {
            // Re-opened while the close was in flight, so leave everything up.
            if state.borrow().settings_panel_open {
                return;
            }
            let (revealer, scrim) = {
                let ui = state.borrow();
                (ui.settings_revealer.clone(), ui.settings_scrim.clone())
            };
            if let Some(revealer) = revealer.as_ref() {
                revealer.set_visible(false);
            }
            if let Some(scrim) = scrim.as_ref() {
                scrim.set_visible(false);
                scrim.add_css_class("settings-closed");
            }
        });
    }
}

fn set_settings_button_active(state: &Rc<RefCell<UiState>>, active: bool) {
    let Some(anchor) = state.borrow().settings_anchor.clone() else {
        return;
    };
    let Ok(button) = anchor.downcast::<gtk::ToggleButton>() else {
        return;
    };
    if button.is_active() != active {
        button.set_active(active);
    }
}

/// Position the panel inside the window. Runs on open and on every layout
/// watcher tick while the panel is up, so a resize cannot leave it hanging off
/// the edge of the window.
pub(crate) fn sync_settings_panel(state: &Rc<RefCell<UiState>>) {
    let ui = state.borrow();
    let (Some(overlay), Some(revealer), Some(panel), Some(scroll), Some(scrim)) = (
        ui.window_overlay.as_ref(),
        ui.settings_revealer.as_ref(),
        ui.settings_panel.as_ref(),
        ui.settings_scroll.as_ref(),
        ui.settings_scrim.as_ref(),
    ) else {
        return;
    };
    if !ui.settings_panel_open {
        return;
    }

    let width = overlay.width();
    let height = overlay.height();
    if width <= 0 || height <= 0 {
        return;
    }

    let panel_width = (width - 2 * SETTINGS_PANEL_EDGE)
        .min(SETTINGS_PANEL_WIDTH)
        .max(SETTINGS_PANEL_MIN_WIDTH.min((width - 2 * SETTINGS_PANEL_EDGE).max(1)));

    // Anchor to the toolbar button when it is in this surface. In the narrow
    // tiers the button lives inside the compact popover, which is a different
    // surface, so the player bar's bottom edge is the anchor instead.
    let (anchor_x, anchor_bottom) = settings_anchor_point(&ui, overlay);

    let top = (anchor_bottom.round() as i32 + SETTINGS_PANEL_GAP)
        .min((height - SETTINGS_PANEL_EDGE * 4).max(SETTINGS_PANEL_EDGE))
        .max(SETTINGS_PANEL_EDGE);
    let left = ((anchor_x.round() as i32) - panel_width)
        .min((width - panel_width - SETTINGS_PANEL_EDGE).max(SETTINGS_PANEL_EDGE))
        .max(SETTINGS_PANEL_EDGE);

    // Sized explicitly rather than left to the overlay's align handling, so the
    // click-catcher is guaranteed to cover the whole window.
    scrim.set_size_request(width, height);

    let max_content = (height - top - SETTINGS_PANEL_EDGE).max(120);
    panel.set_size_request(panel_width, -1);
    scroll.set_max_content_height(max_content);

    revealer.set_margin_top(top);
    revealer.set_margin_start(left);
    revealer.set_margin_end((width - left - panel_width).max(0));
    revealer.set_margin_bottom(0);

    if debug_layout() {
        let natural_height = panel.measure(Orientation::Vertical, panel_width).1;
        let panel_height = natural_height.min(max_content);
        let inside = left >= SETTINGS_PANEL_EDGE
            && top >= SETTINGS_PANEL_EDGE
            && left + panel_width <= width - SETTINGS_PANEL_EDGE
            && top + panel_height <= height - SETTINGS_PANEL_EDGE;
        eprintln!(
            "settings: panel {panel_width}x{panel_height} at ({left},{top}) in window {width}x{height} inside={inside}"
        );
    }
}

/// The point the panel hangs off: the settings button's right/bottom corner when
/// it is a descendant of the overlay, otherwise the player bar's right/bottom.
fn settings_anchor_point(ui: &UiState, overlay: &gtk::Overlay) -> (f64, f64) {
    if !ui.compact_actions_moved
        && let Some(anchor) = ui.settings_anchor.as_ref()
        && let Some(rect) = anchor.compute_bounds(overlay)
        && rect.width() > 0.0
        && rect.height() > 0.0
    {
        return (
            (rect.x() + rect.width()) as f64,
            (rect.y() + rect.height()) as f64,
        );
    }

    let bar_bottom = ui
        .player_bar
        .as_ref()
        .and_then(|bar| bar.compute_bounds(overlay))
        .map(|rect| (rect.y() + rect.height()) as f64)
        .unwrap_or(SETTINGS_PANEL_EDGE as f64);

    let width = overlay.width() as f64;
    (width - SETTINGS_PANEL_EDGE as f64, bar_bottom)
}

fn window_of(widget: &gtk::Widget) -> Option<gtk::Window> {
    widget
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
}

fn switch_row(title: &str, active: bool) -> (gtk::Box, gtk::Switch) {
    let row = gtk::Box::new(Orientation::Horizontal, 12);
    row.add_css_class("settings-switch-row");
    row.set_margin_top(2);
    row.set_margin_bottom(6);
    row.set_margin_start(6);
    row.set_margin_end(6);

    let title_label = label(title, "settings-menu-label");
    title_label.set_hexpand(true);
    title_label.set_halign(Align::Start);

    let switch = gtk::Switch::builder()
        .active(active)
        .valign(Align::Center)
        .build();

    row.append(&title_label);
    row.append(&switch);

    (row, switch)
}

/// The menu itself. Same rows as the old popover, minus the popover.
pub(crate) fn build_settings_content(state: Rc<RefCell<UiState>>) -> gtk::Box {
    let menu = gtk::Box::new(Orientation::Vertical, 4);
    menu.add_css_class("settings-menu-box");
    menu.set_margin_top(2);
    menu.set_margin_bottom(2);
    menu.set_margin_start(2);
    menu.set_margin_end(2);

    let (keep_row, keep_switch) = switch_row(
        "Keep playing while closed",
        state.borrow().keep_playing_while_closed,
    );
    {
        let state = state.clone();
        keep_switch.connect_active_notify(move |switch| {
            set_keep_playing_while_closed(&state, switch.is_active());
        });
    }
    menu.append(&keep_row);

    let (anim_row, anim_switch) = switch_row("Animations", state.borrow().animations_enabled);
    {
        let state = state.clone();
        let menu_handle = menu.clone();
        anim_switch.connect_active_notify(move |switch| {
            let enabled = switch.is_active();
            set_animations_enabled(&state, enabled);
            if let Some(window) = window_of(menu_handle.upcast_ref()) {
                if enabled {
                    window.remove_css_class("no-animations");
                } else {
                    window.add_css_class("no-animations");
                }
            }
        });
    }
    menu.append(&anim_row);

    let font_row = gtk::Box::new(Orientation::Horizontal, 12);
    font_row.add_css_class("settings-switch-row");
    font_row.set_margin_top(2);
    font_row.set_margin_bottom(6);
    font_row.set_margin_start(6);
    font_row.set_margin_end(6);
    let font_label = label("Font Style", "settings-menu-label");
    font_label.set_hexpand(true);
    font_label.set_halign(Align::Start);
    let font_btn_default = gtk::ToggleButton::builder()
        .icon_name("font-x-generic-symbolic")
        .tooltip_text("Default")
        .active(!state.borrow().font_mono)
        .build();
    font_btn_default.add_css_class("font-style-toggle");
    let font_btn_mono = gtk::ToggleButton::builder()
        .icon_name("utilities-terminal-symbolic")
        .tooltip_text("Monospace")
        .active(state.borrow().font_mono)
        .group(&font_btn_default)
        .build();
    font_btn_mono.add_css_class("font-style-toggle");
    let font_toggle_box = gtk::Box::new(Orientation::Horizontal, 0);
    font_toggle_box.add_css_class("linked");
    font_toggle_box.append(&font_btn_default);
    font_toggle_box.append(&font_btn_mono);
    {
        let state = state.clone();
        let menu_handle = menu.clone();
        font_btn_mono.connect_toggled(move |btn| {
            let mono = btn.is_active();
            set_font_mono(&state, mono);
            if let Some(window) = window_of(menu_handle.upcast_ref()) {
                if mono {
                    window.add_css_class("font-mono");
                } else {
                    window.remove_css_class("font-mono");
                }
            }
        });
    }
    font_row.append(&font_label);
    font_row.append(&font_toggle_box);
    menu.append(&font_row);

    menu.append(&gtk::Separator::new(Orientation::Horizontal));

    let refresh = menu_item_button("view-refresh-symbolic", "Refresh library");
    refresh.set_sensitive(false);
    {
        let state = state.clone();
        refresh.connect_clicked(move |button| {
            close_settings_panel(&state);
            refresh_jellyfin_library(state.clone(), button.clone());
        });
    }
    state.borrow_mut().refresh_button = Some(refresh.clone());
    menu.append(&refresh);

    let shortcuts = menu_item_button(
        "preferences-desktop-keyboard-shortcuts-symbolic",
        "Keyboard shortcuts",
    );
    {
        let state = state.clone();
        shortcuts.connect_clicked(move |button| {
            close_settings_panel(&state);
            if let Some(window) = window_of(button.upcast_ref()) {
                show_keyboard_shortcuts(&window);
            }
        });
    }
    menu.append(&shortcuts);

    let about = menu_item_button("help-about-symbolic", "About gTunes");
    {
        let state = state.clone();
        about.connect_clicked(move |button| {
            close_settings_panel(&state);
            if let Some(window) = window_of(button.upcast_ref()) {
                show_about_window(&window);
            }
        });
    }
    menu.append(&about);

    let reset = menu_item_button("edit-delete-symbolic", "Reset database and cache");
    {
        let state = state.clone();
        reset.connect_clicked(move |button| {
            close_settings_panel(&state);
            if let Some(window) = window_of(button.upcast_ref()) {
                confirm_database_reset(&window, state.clone());
            }
        });
    }
    menu.append(&reset);
    menu.append(&gtk::Separator::new(Orientation::Horizontal));

    let quit = menu_item_button("application-exit-symbolic", "Quit");
    quit.add_css_class("destructive-action");
    {
        let state = state.clone();
        quit.connect_clicked(move |button| {
            if let Some(window) = window_of(button.upcast_ref()) {
                quit_application(&window, &state);
            }
        });
    }
    menu.append(&quit);

    menu
}
