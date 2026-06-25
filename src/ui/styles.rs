pub const APP_CSS: &str = r#"
window {
  background: @window_bg_color;
  color: @window_fg_color;
}

.app-root {
  background: @window_bg_color;
}

.player-bar {
  padding: 14px 16px;
  border-bottom: 1px solid @borders;
  background: @headerbar_bg_color;
}

.cover {
  border-radius: 8px;
  box-shadow: 0 1px 3px alpha(@window_fg_color, .18);
}

.sidebar-cover-frame {
  margin-top: 12px;
  border-radius: 8px;
  background: @card_bg_color;
  box-shadow: 0 1px 2px alpha(@window_fg_color, .10);
}

.sidebar-cover {
  border-radius: 8px;
}

.now-title {
  font-size: 15px;
  font-weight: 800;
  color: @window_fg_color;
}

.meta {
  font-size: 11px;
  color: alpha(@window_fg_color, .68);
}

.transport {
  padding: 4px;
  border: 1px solid @borders;
  border-radius: 999px;
  background: @card_bg_color;
  box-shadow: 0 1px 2px alpha(@window_fg_color, .06);
}

.icon-button {
  min-width: 34px;
  min-height: 34px;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: @window_fg_color;
  transition: background-color 140ms ease-out, color 140ms ease-out, box-shadow 140ms ease-out;
}

.icon-button:hover {
  background: alpha(@window_fg_color, .08);
}

.icon-button:active {
  background: alpha(@window_fg_color, .13);
}

.toolbar-button {
  min-width: 28px;
  min-height: 28px;
}

.settings-menu-button,
.settings-menu-button > button {
  min-width: 28px;
  min-height: 28px;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: transparent;
  box-shadow: none;
  color: @window_fg_color;
}

.settings-menu-button:hover,
.settings-menu-button > button:hover {
  background: alpha(@window_fg_color, .08);
}

.settings-menu-button:checked,
.settings-menu-button > button:checked,
.settings-menu-button:active,
.settings-menu-button > button:active {
  background: alpha(@window_fg_color, .13);
  box-shadow: none;
}

.settings-popover-menu {
  background: transparent;
}

.settings-switch-row {
  min-height: 36px;
}

.settings-menu-item {
  min-height: 38px;
  padding: 0;
  border-radius: 6px;
}

.settings-menu-item image {
  color: alpha(@window_fg_color, .72);
}

.settings-menu-label {
  font-size: 13px;
}

.shortcuts-dialog {
  background: @window_bg_color;
}

.shortcuts-header {
  margin-bottom: 10px;
}

.shortcuts-group-title {
  font-size: 13px;
  font-weight: 700;
  color: alpha(@window_fg_color, .72);
}

.shuffle-toggle.shuffle-off {
  color: alpha(@window_fg_color, .72);
}

.shuffle-toggle.shuffle-on {
  color: @accent_fg_color;
  background: @accent_bg_color;
}

.shuffle-toggle.shuffle-on:hover {
  background: mix(@accent_bg_color, @accent_fg_color, .12);
}

.play-button {
  min-width: 42px;
  min-height: 42px;
  color: @accent_fg_color;
  background: @accent_bg_color;
  box-shadow: 0 1px 2px alpha(@window_fg_color, .14);
}

.play-button:hover {
  box-shadow: 0 2px 6px alpha(@window_fg_color, .16);
}

.wave-card {
  min-height: 116px;
  padding: 12px 16px 10px;
  border: 1px solid @borders;
  border-radius: 8px;
  background: @card_bg_color;
  box-shadow: 0 1px 2px alpha(@window_fg_color, .08);
}

.wave-marker {
  padding: 2px 7px;
  border-radius: 999px;
  background: alpha(@window_fg_color, .08);
  color: alpha(@window_fg_color, .72);
  font-size: 10px;
  font-weight: 800;
}

.mono {
  color: alpha(@window_fg_color, .62);
  font-size: 10px;
  font-feature-settings: "tnum";
}

.search {
  min-width: 0;
  min-height: 35px;
  border-radius: 999px;
}

.main-paned {
  background: @window_bg_color;
}

.sidebar {
  min-width: 184px;
  padding: 14px 10px;
  border-right: 1px solid @borders;
  background: @sidebar_bg_color;
}

.section-title {
  margin: 4px 8px 8px;
  color: alpha(@window_fg_color, .72);
  font-size: 11px;
  font-weight: 900;
}

.section-label {
  margin: 15px 8px 5px;
  color: alpha(@window_fg_color, .55);
  font-size: 10px;
  font-weight: 900;
}

.nav-list {
  background: transparent;
}

.nav-list row {
  min-height: 38px;
  padding: 0 8px;
  border-radius: 7px;
  transition: background-color 140ms ease-out, color 140ms ease-out;
}

.nav-list row:hover {
  background: alpha(@window_fg_color, .06);
}

.nav-list row:selected {
  color: @accent_fg_color;
  background: @accent_bg_color;
}

.nav-list row:selected image {
  color: @accent_fg_color;
}

.count {
  color: alpha(@window_fg_color, .54);
  font-size: 10px;
  font-feature-settings: "tnum";
}

.nav-list row:selected .count {
  color: alpha(@accent_fg_color, .82);
}

.status-dot {
  min-width: 8px;
  min-height: 8px;
  border-radius: 999px;
  background: @success_color;
}

.content {
  background: @view_bg_color;
}

.content-header {
  min-height: 64px;
  padding: 12px 16px;
  border-bottom: 1px solid @borders;
  background: @window_bg_color;
}

.page-title {
  color: @window_fg_color;
  font-size: 21px;
  font-weight: 900;
}

.connection-card {
  margin: 14px 16px 12px;
  padding: 14px;
  border: 1px solid @borders;
  border-radius: 8px;
  background: @card_bg_color;
  box-shadow: 0 1px 2px alpha(@window_fg_color, .06);
}

.connection-card entry {
  min-height: 34px;
}

.connection-button {
  min-height: 34px;
  padding: 0 14px;
  border-radius: 8px;
  font-weight: 800;
}

.bottom-reconnect-button {
  min-height: 28px;
  padding: 0 12px;
  font-size: 12px;
}

.reconnect-dialog-content {
  min-width: 354px;
}

.reconnect-summary {
  padding: 10px 12px;
  border: 1px solid @borders;
  border-radius: 8px;
  background: alpha(@window_fg_color, .035);
}

.reconnect-summary-row {
  min-height: 24px;
}

.reconnect-summary-name {
  font-size: 11px;
  font-weight: 800;
  color: alpha(@window_fg_color, .58);
}

.reconnect-summary-value {
  font-size: 12px;
  color: @window_fg_color;
}

.reconnect-password {
  min-height: 38px;
}

.reconnect-status {
  min-height: 18px;
}

.detail-header {
  min-height: 64px;
  padding: 10px 16px;
  border-bottom: 1px solid @borders;
  background: @window_bg_color;
}

.collection-scroll {
  background: @view_bg_color;
}

.collection-grid {
  padding: 18px;
  background: @view_bg_color;
}

.album-grid flowboxchild,
.album-grid flowboxchild:hover,
.album-grid flowboxchild:active,
.album-grid flowboxchild:selected,
.album-grid flowboxchild:focus,
.album-grid flowboxchild:focus-visible,
.artist-grid flowboxchild,
.artist-grid flowboxchild:hover,
.artist-grid flowboxchild:active,
.artist-grid flowboxchild:selected,
.artist-grid flowboxchild:focus,
.artist-grid flowboxchild:focus-visible {
  padding: 0;
  border: 0;
  border-radius: 0;
  box-shadow: none;
  background: transparent;
}

.collection-tile {
  min-width: 184px;
  min-height: 238px;
  padding: 8px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  transition: background-color 140ms ease-out, border-color 140ms ease-out, box-shadow 140ms ease-out;
}

.album-tile {
  min-width: 184px;
}

.collection-tile:hover {
  border-color: alpha(@borders, .72);
  background: @card_bg_color;
  box-shadow: 0 1px 3px alpha(@window_fg_color, .08);
}

.collection-tile:focus-visible {
  border-color: alpha(@accent_bg_color, .58);
  outline: 2px solid alpha(@accent_bg_color, .36);
  outline-offset: -2px;
  background: @card_bg_color;
}

.album-tile,
.album-tile:hover,
.album-tile:active,
.album-tile:focus,
.album-tile:focus-visible {
  background-clip: padding-box;
}

.collection-tile:active {
  border-color: alpha(@accent_bg_color, .42);
  background: alpha(@accent_bg_color, .12);
}

.collection-tile.return-highlight {
  border-color: alpha(@accent_bg_color, .76);
  background: alpha(@accent_bg_color, .18);
  box-shadow: 0 0 0 2px alpha(@accent_bg_color, .24), 0 2px 10px alpha(@window_fg_color, .12);
}

.collection-art {
  border-radius: 8px;
  background: @card_bg_color;
}

.album-art-frame {
  min-width: 168px;
  min-height: 168px;
  border-radius: 8px;
  background: @card_bg_color;
  box-shadow: inset 0 0 0 1px alpha(@borders, .72);
  transition: box-shadow 140ms ease-out;
}

.album-tile:hover .album-art-frame,
.album-tile:focus .album-art-frame,
.album-tile:focus-visible .album-art-frame {
  box-shadow:
    inset 0 0 0 1px alpha(@accent_bg_color, .58),
    0 2px 8px alpha(@window_fg_color, .14);
}

.album-tile:active .album-art-frame {
  box-shadow:
    inset 0 0 0 1px alpha(@accent_bg_color, .72),
    0 1px 4px alpha(@window_fg_color, .12);
}

.album-art {
  min-width: 168px;
  min-height: 168px;
  border-radius: 8px;
}

.artist-art {
  min-width: 148px;
  min-height: 148px;
  border-radius: 8px;
  box-shadow: inset 0 0 0 1px alpha(@borders, .72);
  transition: box-shadow 140ms ease-out;
}

.artist-tile:hover .artist-art,
.artist-tile:focus .artist-art,
.artist-tile:focus-visible .artist-art {
  box-shadow:
    inset 0 0 0 1px alpha(@accent_bg_color, .50),
    0 2px 8px alpha(@window_fg_color, .12);
}

.artist-placeholder {
  color: alpha(@window_fg_color, .48);
  background: alpha(@window_fg_color, .08);
}

.collection-title {
  color: @window_fg_color;
  font-size: 12px;
  font-weight: 800;
}

.collection-subtitle {
  color: alpha(@window_fg_color, .64);
  font-size: 11px;
}

.collection-empty {
  padding: 18px;
}

.track-empty-state {
  margin: 18px;
  padding: 16px;
  border: 1px dashed @borders;
  border-radius: 8px;
  background: alpha(@card_bg_color, .72);
}

.track-list {
  background: @view_bg_color;
}

.track-list header button {
  min-height: 36px;
  padding: 5px 12px 6px;
  color: alpha(@window_fg_color, .62);
  font-size: 11px;
  font-weight: 800;
  background: @window_bg_color;
  border-bottom: 1px solid @borders;
}

.track-list header button label {
  padding-top: 1px;
  letter-spacing: 0;
}

.track-list header button:hover {
  color: alpha(@window_fg_color, .82);
  background: alpha(@window_fg_color, .04);
}

.track-list header button:checked {
  color: @window_fg_color;
  background: alpha(@accent_bg_color, .07);
}

.track-list row {
  min-height: 46px;
  transition: background-color 120ms ease-out;
}

.track-list row:hover {
  background: alpha(@accent_bg_color, .08);
}

.track-list row:selected {
  background: alpha(@accent_bg_color, .14);
}

.track-list row:focus-visible {
  outline: 2px solid alpha(@accent_bg_color, .32);
  outline-offset: -2px;
}

.track-cell {
  padding: 9px 12px 7px;
  color: @window_fg_color;
  font-size: 12px;
  font-weight: 400;
}

.track-time-cell {
  padding-left: 6px;
  padding-right: 6px;
}

.track-title-cell {
  padding-left: 16px;
}

.track-title {
  font-weight: 600;
}

.now-playing-indicator {
  color: @accent_bg_color;
  transition: opacity 160ms ease-out;
}

.radio-page {
  padding: 18px 18px 76px;
  background: @view_bg_color;
}

.radio-header {
  min-height: 46px;
  padding: 0 2px 4px;
}

.radio-grid {
  background: transparent;
}

.radio-grid flowboxchild,
.radio-grid flowboxchild:hover,
.radio-grid flowboxchild:active,
.radio-grid flowboxchild:selected,
.radio-grid flowboxchild:focus,
.radio-grid flowboxchild:focus-visible {
  padding: 0;
  border: 0;
  border-radius: 0;
  box-shadow: none;
  background: transparent;
}

.radio-station-card {
  min-width: 154px;
  min-height: 154px;
  padding: 10px;
  border: 1px solid @borders;
  border-radius: 8px;
  background: @card_bg_color;
  box-shadow: 0 1px 2px alpha(@window_fg_color, .06);
  transition: background-color 140ms ease-out, border-color 140ms ease-out, box-shadow 140ms ease-out;
}

.radio-station-card:hover {
  border-color: alpha(@accent_bg_color, .44);
  background: mix(@card_bg_color, @accent_bg_color, .04);
  box-shadow: 0 2px 8px alpha(@window_fg_color, .10);
}

.radio-station-card-playing {
  border-color: alpha(@accent_bg_color, .72);
  background: mix(@card_bg_color, @accent_bg_color, .10);
}

.radio-receiver-icon {
  color: @accent_bg_color;
}

.radio-card-icon {
  margin-top: 2px;
  min-height: 48px;
  color: @accent_bg_color;
  font-family: "Symbols Nerd Font Propo", "Symbols Nerd Font", "Noto Sans Symbols 2", "Noto Sans Symbols2", sans-serif;
  font-size: 32px;
  font-weight: 700;
}

.radio-station-title {
  color: @window_fg_color;
  font-size: 14px;
  font-weight: 900;
}

.radio-playing-badge {
  min-height: 24px;
  padding: 0 9px;
  border-radius: 999px;
  color: @accent_fg_color;
  background: @accent_bg_color;
  font-size: 10px;
  font-weight: 900;
}

.radio-add-popover {
  background: transparent;
}

.radio-add-panel {
  min-width: 284px;
  padding: 14px 16px 16px;
  border: 1px solid @borders;
  border-radius: 8px;
  background: @card_bg_color;
  box-shadow: 0 1px 2px alpha(@window_fg_color, .06);
}

.radio-add-panel entry {
  min-height: 34px;
}

.radio-add-panel .connection-button {
  min-height: 36px;
  padding: 0 16px;
}

.radio-add-fab,
.radio-add-fab > button {
  min-width: 42px;
  min-height: 42px;
  padding: 0;
  border-radius: 999px;
  color: @accent_fg_color;
  background: @accent_bg_color;
  box-shadow: 0 2px 8px alpha(@window_fg_color, .18);
}

.radio-add-fab:hover,
.radio-add-fab > button:hover {
  background: mix(@accent_bg_color, @accent_fg_color, .12);
}

.radio-remove-button {
  min-width: 28px;
  min-height: 28px;
  padding: 0;
  border-radius: 999px;
  color: alpha(@window_fg_color, .72);
}

.radio-remove-button:hover {
  color: @destructive_color;
  background: alpha(@destructive_color, .10);
}

.quality {
  padding: 3px 8px;
  border-radius: 999px;
  color: @accent_fg_color;
  background: @accent_bg_color;
  font-size: 10px;
  font-weight: 900;
}

.rail-header {
  padding: 14px 16px 13px;
  border-bottom: 1px solid @borders;
}

.rail-title {
  color: @window_fg_color;
  font-size: 15px;
  font-weight: 900;
}

.placeholder {
  margin: 16px;
  padding: 16px;
  border: 1px dashed @borders;
  border-radius: 8px;
  background: alpha(@card_bg_color, .72);
}

.placeholder-icon {
  color: alpha(@window_fg_color, .62);
}

.queue-card {
  margin: 12px 0 0;
  padding: 10px;
  border: 1px solid @borders;
  border-radius: 8px;
  background: @card_bg_color;
  min-width: 0;
}

.queue-link {
  min-height: 0;
  padding: 2px 0 6px;
  border-radius: 6px;
  background: transparent;
}

.queue-link:hover {
  background: alpha(@accent_bg_color, .08);
}

.queue-link:focus-visible {
  outline: 2px solid alpha(@accent_bg_color, .55);
  outline-offset: -2px;
}

.queue-scroll {
  min-height: 0;
}

.queue-row {
  min-height: 38px;
  min-width: 0;
  padding: 4px 6px;
  border-radius: 6px;
  border-bottom: 1px solid alpha(@borders, .75);
  background: transparent;
  transition: background-color 120ms ease-out, box-shadow 120ms ease-out;
}

.queue-title {
  color: @window_fg_color;
  font-size: 10px;
  font-weight: 700;
}

.queue-artist {
  color: alpha(@window_fg_color, .60);
  font-size: 9px;
}

.queue-row:hover {
  background: alpha(@accent_bg_color, .10);
}

.queue-row:active {
  background: alpha(@accent_bg_color, .18);
}

.queue-row:focus-visible {
  outline: 2px solid alpha(@accent_bg_color, .55);
  outline-offset: -2px;
}

.queue-row label {
  padding: 0;
}

.next-up-page {
  min-width: 0;
}

.next-up-empty {
  margin-top: 6px;
}

.next-up-list {
  min-width: 0;
}

.next-up-row {
  min-height: 72px;
  padding: 12px 16px;
  border: 1px solid alpha(@borders, .88);
  border-radius: 12px;
  background: alpha(@card_bg_color, .94);
  transition: background-color 120ms ease-out, border-color 120ms ease-out, box-shadow 120ms ease-out;
}

.next-up-row:hover {
  background: alpha(@accent_bg_color, .08);
}

.next-up-row.dragging {
  opacity: .72;
}

.next-up-row.drop-before {
  border-top-color: @accent_bg_color;
  box-shadow: inset 0 3px 0 0 alpha(@accent_bg_color, .75);
}

.next-up-row.drop-after {
  border-bottom-color: @accent_bg_color;
  box-shadow: inset 0 -3px 0 0 alpha(@accent_bg_color, .75);
}

.next-up-index {
  min-width: 20px;
  color: alpha(@window_fg_color, .58);
  font-size: 11px;
  font-weight: 800;
}

.next-up-art {
  min-width: 48px;
  min-height: 48px;
  border-radius: 8px;
  background: alpha(@headerbar_bg_color, .7);
}

.next-up-text {
  min-width: 0;
}

.next-up-title {
  color: @window_fg_color;
  font-size: 14px;
  font-weight: 800;
}

.next-up-artist {
  color: alpha(@window_fg_color, .65);
  font-size: 12px;
}

.next-up-trailing {
  min-width: 64px;
}

.next-up-time {
  min-width: 34px;
  color: alpha(@window_fg_color, .70);
}

.next-up-handle {
  min-width: 16px;
  color: alpha(@window_fg_color, .55);
}

.next-up-handle:hover {
  color: @window_fg_color;
}

.bottom-bar {
  min-height: 42px;
  padding: 0 16px;
  border-top: 1px solid @borders;
  background: @headerbar_bg_color;
}
"#;

pub fn load() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(APP_CSS);

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
