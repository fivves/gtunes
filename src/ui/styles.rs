pub const APP_CSS: &str = r#"
window {
  background: @window_bg_color;
  color: @window_fg_color;
}

.app-root {
  background: @window_bg_color;
}

.player-bar {
  padding: 12px;
  border-bottom: 1px solid @borders;
  background: @headerbar_bg_color;
}

.cover {
  border-radius: 8px;
  box-shadow: 0 1px 3px alpha(@window_fg_color, .18);
}

.sidebar-cover-frame {
  margin-top: 10px;
  border-radius: 8px;
  background: @card_bg_color;
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
  padding: 3px;
  border: 1px solid @borders;
  border-radius: 999px;
  background: @card_bg_color;
}

.icon-button {
  min-width: 34px;
  min-height: 34px;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: @window_fg_color;
}

.icon-button:hover {
  background: alpha(@window_fg_color, .08);
}

.toolbar-button {
  min-width: 36px;
  min-height: 36px;
}

.action-strip {
  padding: 3px;
  border: 1px solid @borders;
  border-radius: 999px;
  background: alpha(@window_fg_color, .035);
}

.shuffle-toggle {
  min-width: 36px;
  padding: 0;
  border: 0;
  background: transparent;
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

.shuffle-state-label {
  font-size: 10px;
  font-weight: 900;
}

.play-button {
  min-width: 42px;
  min-height: 42px;
  color: @accent_fg_color;
  background: @accent_bg_color;
}

.wave-card {
  min-height: 116px;
  padding: 10px 14px 8px;
  border: 1px solid @borders;
  border-radius: 8px;
  background: @card_bg_color;
  box-shadow: 0 1px 2px alpha(@window_fg_color, .06);
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
  min-width: 220px;
  min-height: 35px;
  border-radius: 999px;
}

.main-paned {
  background: @window_bg_color;
}

.sidebar {
  min-width: 184px;
  padding: 12px 10px;
  border-right: 1px solid @borders;
  background: @sidebar_bg_color;
}

.section-title {
  margin: 5px 8px 7px;
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
  min-height: 36px;
  padding: 0 8px;
  border-radius: 7px;
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
  padding: 12px 14px;
  border-bottom: 1px solid @borders;
  background: @window_bg_color;
}

.page-title {
  color: @window_fg_color;
  font-size: 25px;
  font-weight: 900;
}

.connection-card {
  margin: 12px 14px;
  padding: 12px;
  border: 1px solid @borders;
  border-radius: 8px;
  background: @card_bg_color;
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

.detail-header {
  min-height: 64px;
  padding: 10px 14px;
  border-bottom: 1px solid @borders;
  background: @window_bg_color;
}

.collection-scroll {
  background: @view_bg_color;
}

.collection-grid {
  padding: 16px;
  background: @view_bg_color;
}

.collection-tile {
  min-width: 156px;
  min-height: 228px;
  padding: 8px;
  border-radius: 8px;
  background: transparent;
}

.album-tile {
  min-width: 168px;
  min-height: 168px;
  padding: 0;
  border: 0;
  box-shadow: none;
  background: transparent;
}

.collection-tile:hover {
  background: alpha(@accent_bg_color, .10);
}

.album-tile:hover {
  background: transparent;
}

.collection-tile:active {
  background: alpha(@accent_bg_color, .18);
}

.album-tile:active {
  background: transparent;
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
}

.album-art {
  min-width: 168px;
  min-height: 168px;
  border-radius: 8px;
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

.track-list {
  background: @view_bg_color;
}

.track-list header button {
  min-height: 32px;
  padding: 0 10px;
  color: alpha(@window_fg_color, .66);
  font-size: 10px;
  font-weight: 700;
  background: alpha(@window_fg_color, .06);
  border-bottom: 1px solid @borders;
}

.track-list row {
  min-height: 44px;
}

.track-list row:hover {
  background: alpha(@accent_bg_color, .08);
}

.track-list row:selected {
  background: alpha(@accent_bg_color, .14);
}

.track-cell {
  padding: 8px 10px 6px;
  color: @window_fg_color;
  font-size: 12px;
  font-weight: 400;
}

.track-time-cell {
  padding-left: 6px;
  padding-right: 6px;
}

.track-title-cell {
  padding-left: 14px;
}

.track-title {
  font-weight: 600;
}

.now-playing-indicator {
  color: @accent_bg_color;
}

.quality {
  padding: 3px 8px;
  border-radius: 999px;
  color: @accent_fg_color;
  background: @accent_bg_color;
  font-size: 10px;
  font-weight: 900;
}

.context-rail {
  min-width: 300px;
  border-left: 1px solid @borders;
  background: @window_bg_color;
}

.rail-header {
  padding: 13px 14px 12px;
  border-bottom: 1px solid @borders;
}

.rail-title {
  color: @window_fg_color;
  font-size: 15px;
  font-weight: 900;
}

.placeholder {
  margin: 14px;
  padding: 15px;
  border: 1px dashed @borders;
  border-radius: 8px;
  background: alpha(@card_bg_color, .72);
}

.placeholder-icon {
  color: alpha(@window_fg_color, .62);
}

.queue-card {
  margin: 10px 0 0;
  padding: 8px;
  border: 1px solid @borders;
  border-radius: 8px;
  background: @card_bg_color;
  min-width: 0;
}

.queue-scroll {
  min-height: 0;
}

.queue-row {
  min-height: 34px;
  min-width: 0;
  padding: 3px 5px;
  border-radius: 6px;
  border-bottom: 1px solid alpha(@borders, .75);
  background: transparent;
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

.bottom-bar {
  min-height: 40px;
  padding: 0 14px;
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
