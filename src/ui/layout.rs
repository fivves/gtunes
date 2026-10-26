//! Window layout tiers.
//!
//! The window grew around fixed minimums: a 240px sidebar plus a track table
//! whose four columns need 236 + 136 + 196 + 66 = 634px, so the body wants about
//! 874px before the table shows all four without squeezing them. Under that the
//! app clipped instead of reflowing, and because the sidebar also stacks a
//! five-row nav list, the "Next Up" card and a 220px square cover art, short
//! windows chopped its bottom off as well. Those are two different problems, so the class has two
//! independent axes: width decides density (columns, nav labels, player bar
//! furniture) and height decides the sidebar's vertical stack.
//!
//! Thresholds live here and nowhere else, so retuning the behaviour is a
//! constant change. `classify` is pure and covered by tests; everything else
//! is apply-on-change plumbing.
//!
//! Why a coarse timer instead of `AdwBreakpoint`: breakpoints set GObject
//! properties, so each one would have to restate the whole CSS class list and
//! the last applying breakpoint would clobber the others. The player bar
//! already watched its own width this way (deliberately, to keep the frame
//! clock idle), so this replaces that watcher rather than adding a second
//! mechanism next to it.

use super::prelude::*;

/// Width thresholds, logical pixels. Every display he drives runs scale 1, and
/// the tiles were measured off a screenshot rather than assumed: full 1896/1920,
/// half 941, quarter 941-961, eighth 466.
///
/// Half is his reference for "looks right", and the quarter still has room for
/// all four columns (961 - 240 sidebar leaves 721 for a 634 table), so the first
/// width tier has to start *below* half. Setting it to the table's old 946 total
/// put half and quarter in the compact tier, which is the mistake that left the
/// quarter clipping instead of reflowing: width was never its problem, height
/// was. That 946 was never a hard minimum either, since the table sits inside a
/// scrolled window: a narrower window scrolled its last few pixels sideways
/// instead of clipping, which is the horizontal scrollbar half size used to have.
/// Prefer reflowing the columns over raising this back up.
pub(crate) const WIDTH_COMPACT_BELOW: i32 = 900;
/// Below this the sidebar collapses to an icon rail and the artist column is
/// replaced by the inline artist.
pub(crate) const WIDTH_NARROW_BELOW: i32 = 780;
/// The smallest tile he uses.
pub(crate) const WIDTH_TINY_BELOW: i32 = 520;

/// Height thresholds. The full layout's own content minimum is ~951px, measured
/// with the tier hooks (913 before the Next Up nav row added a row to the
/// sidebar), so the boundary has to sit ABOVE that: any window shorter than the
/// layout needs shows the full sidebar and then clips. His laptop log said it
/// exactly, `content needs 494x913 in a 905x656 window <-- CLIPS`: a 656px tile
/// was treated as tall and lost its bottom bar.
///
/// 640 was wrong for that reason and had nothing to do with the tiles originally
/// measured. His real tiles are 1020 tall (fullscreen and half) and 494-843
/// (quarter, eighth, and whatever intermediate size a layout lands on), so 1000
/// splits the two groups with margin above the layout's own floor. Keep this
/// above the measured floor and below 1020; it is the one number that has to
/// track the sidebar's content.
///
/// Height is what makes a quarter compact: under this the cover art and the
/// "Next Up" card go. The Next Up nav row is how the queue stays reachable, and
/// dropping the card and the cover art takes what the content needs from ~951px
/// down to 442.
pub(crate) const HEIGHT_SHORT_BELOW: i32 = 1000;
pub(crate) const HEIGHT_TINY_BELOW: i32 = 480;

/// How much horizontal space the collapsed nav keeps.
pub(crate) const SIDEBAR_RAIL_WIDTH: i32 = 56;

/// How often the window size is checked. Matches the interval the player bar
/// used while it owned this job: fine enough to track a drag, cheap enough to
/// leave the frame clock alone.
const WATCH_INTERVAL: Duration = Duration::from_millis(200);

/// With `GTUNES_LAYOUT_DEBUG` set, repeat the current size and tier at least this
/// often, so a log of a resize gesture shows what the app saw even when nothing
/// changed.
const HEARTBEAT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WidthClass {
    Full,
    Compact,
    Narrow,
    Tiny,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeightClass {
    Tall,
    Short,
    Tiny,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LayoutClass {
    pub(crate) width: WidthClass,
    pub(crate) height: HeightClass,
}

impl Default for LayoutClass {
    fn default() -> Self {
        Self {
            width: WidthClass::Full,
            height: HeightClass::Tall,
        }
    }
}

pub(crate) fn classify(width: i32, height: i32) -> LayoutClass {
    let width = if width < WIDTH_TINY_BELOW {
        WidthClass::Tiny
    } else if width < WIDTH_NARROW_BELOW {
        WidthClass::Narrow
    } else if width < WIDTH_COMPACT_BELOW {
        WidthClass::Compact
    } else {
        WidthClass::Full
    };

    let height = if height < HEIGHT_TINY_BELOW {
        HeightClass::Tiny
    } else if height < HEIGHT_SHORT_BELOW {
        HeightClass::Short
    } else {
        HeightClass::Tall
    };

    LayoutClass { width, height }
}

impl LayoutClass {
    /// The nav column becomes an icon-only rail.
    pub(crate) fn uses_icon_rail(self) -> bool {
        matches!(self.width, WidthClass::Narrow | WidthClass::Tiny)
    }

    /// Album only has room at full width.
    pub(crate) fn shows_album_column(self) -> bool {
        self.width == WidthClass::Full
    }

    /// Artist survives until the list gets narrow; past that it moves into the
    /// title row so the information is not lost.
    pub(crate) fn shows_artist_column(self) -> bool {
        matches!(self.width, WidthClass::Full | WidthClass::Compact)
    }

    pub(crate) fn shows_artist_in_row(self) -> bool {
        !self.shows_artist_column()
    }

    /// The "Next Up" card in the sidebar needs height and a real sidebar.
    pub(crate) fn shows_sidebar_card(self) -> bool {
        self.height == HeightClass::Tall && !self.uses_icon_rail()
    }

    /// The same card with the page rule folded in, which is what both owners ask.
    ///
    /// It has two: this tier, which needs a tall window with a real sidebar, and
    /// the page, because the Next Up and Radio pages show that same queue in the
    /// content area and the card only duplicates them. `apply` enforces the tier
    /// on every watcher tick, so a page-only decision is undone within 200ms: that
    /// is how the card came back on the Next Up page in a full screen.
    pub(crate) fn shows_sidebar_queue_card(self, page: LibraryPage) -> bool {
        self.shows_sidebar_card() && !matches!(page, LibraryPage::NextUp | LibraryPage::Radio)
    }

    /// The queue needs exactly one way in and never two: this row stands in for
    /// the sidebar's pill whenever the pill is not on screen.
    pub(crate) fn shows_next_up_nav_row(self) -> bool {
        !self.shows_sidebar_card()
    }

    /// The cover art is a 220px square, so it is the first thing to go when the
    /// window is short. It also never belongs in the icon rail: the rail is a
    /// single column, so the art squeezes the navigation into a lopsided sidebar
    /// instead of sitting under it.
    pub(crate) fn shows_cover_art(self) -> bool {
        self.height == HeightClass::Tall && !self.uses_icon_rail()
    }

    /// Search plus the shuffle/cast/settings row.
    pub(crate) fn shows_player_actions(self) -> bool {
        matches!(self.width, WidthClass::Full | WidthClass::Compact)
    }

    pub(crate) fn shows_playback_status(self) -> bool {
        self.height == HeightClass::Tall && self.width != WidthClass::Tiny
    }

    /// The bottom bar keeps its status text; the longer detail line is the
    /// first thing to drop.
    pub(crate) fn shows_connection_detail(self) -> bool {
        !self.uses_icon_rail()
    }

    /// The classes the root carries, which is what the CSS keys off.
    pub(crate) fn css_classes(self) -> Vec<&'static str> {
        let width = match self.width {
            WidthClass::Full => "layout-full",
            WidthClass::Compact => "layout-compact",
            WidthClass::Narrow => "layout-narrow",
            WidthClass::Tiny => "layout-tiny",
        };
        let height = match self.height {
            HeightClass::Tall => "height-tall",
            HeightClass::Short => "height-short",
            HeightClass::Tiny => "height-tiny",
        };
        vec!["app-root", width, height]
    }
}

/// Watch the root's size and re-apply the class whenever it changes tier.
///
/// Stops itself when the root is gone, so closing the window tears it down.
pub(crate) fn install_layout_watcher(root: &gtk::Box, state: Rc<RefCell<UiState>>) {
    let watched = root.downgrade();
    let last: Cell<Option<LayoutClass>> = Cell::new(None);
    let last_size: Cell<(i32, i32)> = Cell::new((0, 0));
    let started = std::time::Instant::now();
    let last_log: Cell<std::time::Instant> = Cell::new(started);

    gtk::glib::timeout_add_local(WATCH_INTERVAL, move || {
        let Some(root) = watched.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };

        // Measure the toplevel, NOT the root box. GTK always allocates a child
        // at least its minimum size, so the moment the window is shorter than
        // the sidebar's stack the box is handed that bigger minimum and the
        // window clips it. Measuring the box therefore reported the content's
        // ~700px floor no matter how small the window got, the height tier never
        // fired, and the quarter stayed in the full layout with the bottom bar
        // pushed out of view. The toplevel's allocation is the size actually on
        // screen, which is what the tiers are about.
        let window = root.root().and_then(|r| r.downcast::<gtk::Window>().ok());
        let size = match window.as_ref() {
            Some(window) if window.width() > 0 && window.height() > 0 => {
                (window.width(), window.height())
            }
            _ => (root.width(), root.height()),
        };
        let size_changed = size != last_size.get();
        if size_changed {
            last_size.set(size);
        }
        if size.0 <= 0 || size.1 <= 0 {
            return gtk::glib::ControlFlow::Continue;
        }

        let now = std::time::Instant::now();
        let class = forced_class().unwrap_or_else(|| classify(size.0, size.1));
        let previous = last.get();
        let class_changed = previous != Some(class);

        if debug_layout()
            && (size_changed || class_changed || now.duration_since(last_log.get()) >= HEARTBEAT)
        {
            last_log.set(now);
            eprintln!(
                "layout: t={:>7.2}s  {}x{}  ->  {:?}/{:?}  (was {:?})",
                now.duration_since(started).as_secs_f32(),
                size.0,
                size.1,
                class.width,
                class.height,
                previous
            );
        }

        if debug_title()
            && let Some(window) = window.as_ref()
        {
            window.set_title(Some(&format!(
                "gTunes [{}x{} {:?}/{:?}]",
                size.0, size.1, class.width, class.height
            )));
        }

        // Enforced on every tick on purpose: the compositor re-tiles this window
        // as other windows open and close, and other modules re-show widgets this
        // tier owns. Doing it once, on the tier change, leaves the tier undone and
        // the window half-converted.
        apply(&root, &state, class);

        // A menu that is up must stay inside the window, so it re-clamps while
        // open: this is the "window got resized under it" case, and it is a
        // position inside the window rather than the window's own placement, so
        // it cannot fight the compositor.
        if state.borrow().settings_panel_open {
            sync_settings_panel(&state);
        }

        if debug_layout()
            && (size_changed || class_changed)
            && let Some(report) = super::tracklist::track_table_width_report(&state)
        {
            eprintln!("layout: {report}");
        }

        if class_changed {
            last.set(Some(class));
            restructure(&state, previous, class);

            // The number that decides whether anything clips: what the content
            // needs at this width, versus what the window actually gives it.
            if debug_layout() {
                let (min_w, _, _, _) = root.measure(gtk::Orientation::Horizontal, size.1);
                let (_, min_h, _, _) = root.measure(gtk::Orientation::Vertical, size.0);
                let clips = if min_w > size.0 || min_h > size.1 {
                    "  <-- CLIPS"
                } else {
                    ""
                };
                eprintln!(
                    "layout: content needs {min_w}x{min_h} in a {}x{} window{clips}",
                    size.0, size.1
                );
            }
        }
        gtk::glib::ControlFlow::Continue
    });
}

/// Tuning hook: `GTUNES_LAYOUT_FORCE_WINDOW=961x494` opens the window at an exact
/// size so the tiers can be exercised from a script instead of by hand. Only
/// active when the variable is set.
pub(crate) fn forced_window_size() -> Option<(i32, i32)> {
    let raw = std::env::var("GTUNES_LAYOUT_FORCE_WINDOW").ok()?;
    let (width, height) = raw.split_once('x')?;
    Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
}

/// Tuning hook: `GTUNES_LAYOUT_FORCE_CLASS=full/short` pins a layout class, so
/// each tier's content minimum can be measured without needing a window that is
/// actually that size (a floating window cannot be shorter than the content it
/// is currently showing). Only active when the variable is set.
pub(crate) fn forced_class() -> Option<LayoutClass> {
    let raw = std::env::var("GTUNES_LAYOUT_FORCE_CLASS").ok()?;
    let (width, height) = raw.split_once('/')?;
    let width = match width.trim().to_ascii_lowercase().as_str() {
        "full" => WidthClass::Full,
        "compact" => WidthClass::Compact,
        "narrow" => WidthClass::Narrow,
        "tiny" => WidthClass::Tiny,
        _ => return None,
    };
    let height = match height.trim().to_ascii_lowercase().as_str() {
        "tall" => HeightClass::Tall,
        "short" => HeightClass::Short,
        "tiny" => HeightClass::Tiny,
        _ => return None,
    };
    Some(LayoutClass { width, height })
}

/// Diagnostics: `GTUNES_LAYOUT_DEBUG` logs to stderr (the app otherwise throws
/// stderr away, so a run has to be launched with a redirect).
pub(crate) fn debug_layout() -> bool {
    std::env::var_os("GTUNES_LAYOUT_DEBUG").is_some()
}

/// Diagnostics: `GTUNES_LAYOUT_TITLE` additionally puts the measured size and
/// class in the window title, readable from outside with `hyprctl clients`. Kept
/// separate from the stderr log so a logging run does not change what he sees in
/// his bar.
pub(crate) fn debug_title() -> bool {
    std::env::var_os("GTUNES_LAYOUT_TITLE").is_some()
}

/// Idempotent enforcement, with a correction log. A correction means a widget
/// was not already in the state this tier wants, which only happens when another
/// module re-showed something this tier owns.
fn set_visible<W: IsA<gtk::Widget>>(widget: Option<&W>, visible: bool, name: &str) {
    let Some(widget) = widget else {
        return;
    };
    if widget.is_visible() == visible {
        return;
    }
    if debug_layout() {
        eprintln!("layout: corrected {name} -> visible={visible}");
    }
    widget.set_visible(visible);
}

/// Push one class at the widgets.
///
/// Runs on EVERY watcher tick, not only on a tier change. Other modules own some
/// of these widgets too (`collections.rs` sets the "Next Up" card's visibility on
/// every content-view update), and the app re-touches them as the compositor
/// re-tiles the window while other windows open and close. A one-shot apply gets
/// undone with nothing left to put it back, and the tier then looks like it never
/// ran at all. Everything here is cheap and idempotent; the two things that are
/// neither (re-parenting widgets, rebuilding the track model) live in
/// `restructure`.
pub(crate) fn apply(root: &gtk::Box, state: &Rc<RefCell<UiState>>, class: LayoutClass) {
    root.set_css_classes(&class.css_classes());

    {
        let mut ui = state.borrow_mut();
        ui.layout = class;
    }

    let ui = state.borrow();

    if let Some(sidebar) = ui.sidebar.as_ref() {
        let width = if class.uses_icon_rail() {
            SIDEBAR_RAIL_WIDTH
        } else {
            LEFT_SIDEBAR_WIDTH
        };
        sidebar.set_size_request(width, -1);
    }

    set_visible(
        ui.sidebar_header.as_ref(),
        !class.uses_icon_rail(),
        "sidebar header",
    );
    for label in &ui.nav_row_text {
        set_visible(Some(label), !class.uses_icon_rail(), "nav label");
    }
    set_visible(
        ui.sidebar_cover_frame.as_ref(),
        class.shows_cover_art(),
        "cover art",
    );
    set_visible(
        ui.sidebar_queue_card.as_ref(),
        class.shows_sidebar_queue_card(ui.active_page),
        "next up card",
    );
    // Exactly complementary to the card, so the queue always has one way in and
    // never two: the pill in a tall window, the nav row once the pill is gone.
    set_visible(
        ui.nav_queue_row.as_ref(),
        class.shows_next_up_nav_row(),
        "next up nav row",
    );
    set_visible(
        ui.player_actions.as_ref(),
        class.shows_player_actions(),
        "player actions",
    );
    set_visible(
        ui.compact_menu.as_ref(),
        !class.shows_player_actions(),
        "compact menu",
    );
    set_visible(
        Some(&ui.playback_status),
        class.shows_playback_status(),
        "playback status",
    );
    set_visible(
        Some(&ui.connection_detail),
        class.shows_connection_detail(),
        "connection detail",
    );

    for (sort_column, column) in &ui.track_columns {
        let (visible, name) = match sort_column {
            SortColumn::Album => (class.shows_album_column(), "album column"),
            SortColumn::Artist => (class.shows_artist_column(), "artist column"),
            SortColumn::Title => (true, "title column"),
            SortColumn::Duration => (true, "time column"),
        };
        if column.is_visible() != visible {
            if debug_layout() {
                eprintln!("layout: corrected {name} -> visible={visible}");
            }
            column.set_visible(visible);
        }
    }

    drop(ui);
}

/// The parts of a tier change that must not run on every tick: re-parenting the
/// real search box and utility row between the player bar and the compact menu,
/// and rebuilding the track model so the visible rows pick up the inline artist.
fn restructure(state: &Rc<RefCell<UiState>>, previous: Option<LayoutClass>, class: LayoutClass) {
    let actions_move = previous
        .map(|previous| previous.shows_player_actions() != class.shows_player_actions())
        .unwrap_or(true);
    let artist_changes = previous
        .map(|previous| previous.shows_artist_in_row() != class.shows_artist_in_row())
        .unwrap_or(true);

    if actions_move {
        let mut ui = state.borrow_mut();
        move_player_actions(&mut ui, !class.shows_player_actions());
    }

    if artist_changes {
        // This is the model's own refresh path and it keeps scroll and selection
        // intact, so it is safe to run here, but it is too heavy to run on every
        // tick and nothing needs it unless the inline artist changes.
        refresh_track_model(state);
    }

    if debug_layout() {
        eprintln!(
            "layout: applied {class:?} (was {previous:?}) rail={} album={} artist_col={} card={} cover={} actions={} status={}",
            class.uses_icon_rail(),
            class.shows_album_column(),
            class.shows_artist_column(),
            class.shows_sidebar_card(),
            class.shows_cover_art(),
            class.shows_player_actions(),
            class.shows_playback_status()
        );
    }
}

/// Move the search box and the utility row between the player bar and the
/// compact menu's popover. Moving the real widgets means there is one live copy
/// of each rather than a second one that would immediately go stale.
fn move_player_actions(ui: &mut UiState, into_menu: bool) {
    if ui.compact_actions_moved == into_menu {
        return;
    }

    let (Some(actions), Some(content)) =
        (ui.player_actions.clone(), ui.compact_menu_content.clone())
    else {
        return;
    };
    let (Some(search_box), Some(utility_row)) =
        (ui.player_search_box.clone(), ui.player_utility_row.clone())
    else {
        return;
    };

    search_box.unparent();
    utility_row.unparent();

    if into_menu {
        content.append(&search_box);
        content.append(&utility_row);
    } else {
        actions.set_child(Some(&search_box));
        actions.add_overlay(&utility_row);
    }

    ui.compact_actions_moved = into_menu;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_window_is_left_alone() {
        let class = classify(1896, 1020);
        assert_eq!(class.width, WidthClass::Full);
        assert_eq!(class.height, HeightClass::Tall);
        assert!(class.shows_album_column());
        assert!(class.shows_artist_column());
        assert!(class.shows_sidebar_card());
        assert!(class.shows_cover_art());
        assert!(class.shows_player_actions());
        assert!(class.shows_playback_status());
        assert!(!class.uses_icon_rail());
    }

    #[test]
    fn half_window_is_left_alone_too() {
        // His yardstick: at half size it "looks and functions perfectly", so the
        // width tier must not touch it even though it is the narrowest size he
        // calls good.
        for width in [941, 961] {
            let class = classify(width, 1020);
            assert_eq!(class.width, WidthClass::Full);
            assert_eq!(class.height, HeightClass::Tall);
            assert!(!class.uses_icon_rail());
            assert!(class.shows_album_column());
            assert!(class.shows_artist_column());
            assert!(class.shows_sidebar_card());
            assert!(class.shows_cover_art());
            assert!(class.shows_player_actions());
        }
    }

    #[test]
    fn quarter_window_loses_the_sidebar_bulk_but_keeps_its_columns() {
        // Measured off the laptop screenshot (961x494) and the PC tile
        // (941x503). Width still has room for all four columns, so the height is
        // what gives: cover art and queue card go, the Next Up nav row stays,
        // and the bottom bar stays.
        for (width, height) in [(961, 494), (941, 503)] {
            let class = classify(width, height);
            assert_eq!(class.width, WidthClass::Full);
            assert_eq!(class.height, HeightClass::Short);
            assert!(!class.uses_icon_rail());
            assert!(class.shows_album_column());
            assert!(class.shows_artist_column());
            assert!(class.shows_player_actions());
            assert!(!class.shows_cover_art());
            assert!(!class.shows_sidebar_card());
            assert!(!class.shows_playback_status());
            assert!(class.shows_connection_detail());
        }
    }

    #[test]
    fn eighth_window_is_fully_compact_but_keeps_every_control() {
        let class = classify(466, 503);
        assert_eq!(class.width, WidthClass::Tiny);
        assert_eq!(class.height, HeightClass::Short);
        assert!(class.uses_icon_rail());
        assert!(!class.shows_artist_column());
        assert!(class.shows_artist_in_row());
        assert!(!class.shows_cover_art());
        assert!(!class.shows_sidebar_card());
        assert!(!class.shows_player_actions());
        assert!(!class.shows_playback_status());
        assert!(!class.shows_connection_detail());
    }

    #[test]
    fn every_real_tile_lands_in_the_tier_that_fits_it() {
        // His real tiles, measured off his screens. The height tier is the one
        // that was wrong: with HEIGHT_SHORT_BELOW at 640 the 656 and 843 tiles
        // stayed Tall, kept the full sidebar, and clipped. Every one of these
        // must classify to a tier whose content minimum is smaller than the tile.
        // (width, height, expected width class, expected height class)
        let cases = [
            (1896, 1056, WidthClass::Full, HeightClass::Tall), // full screen
            (941, 1020, WidthClass::Full, HeightClass::Tall),  // half
            (961, 494, WidthClass::Full, HeightClass::Short),  // quarter (laptop)
            (941, 503, WidthClass::Full, HeightClass::Short),  // quarter (PC)
            (905, 656, WidthClass::Full, HeightClass::Short),  // the clipping one
            (905, 843, WidthClass::Full, HeightClass::Short),
            (466, 503, WidthClass::Tiny, HeightClass::Short), // eighth
        ];
        for (width, height, wanted_width, wanted_height) in cases {
            let class = classify(width, height);
            assert_eq!(class.width, wanted_width, "{width}x{height} width tier");
            assert_eq!(class.height, wanted_height, "{width}x{height} height tier");
        }
    }

    #[test]
    fn heights_below_the_full_layouts_913_floor_go_short_not_tall() {
        // The full layout's own content minimum is 913px tall, so ANY window
        // shorter than that which is still classified Tall keeps the full
        // sidebar and clips its bottom bar. The boundary must sit above 913, so
        // every height under it has to be Short. This is the regression that made
        // a 905x656 tile clip (`content needs 494x913 in a 905x656 window`).
        for height in [494, 503, 656, 843, 900, 912, 913, 950, 951, 999] {
            assert_eq!(
                classify(905, height).height,
                HeightClass::Short,
                "height {height} under the layout's floor must not stay Tall"
            );
        }
        // And the sizes that were already fine must stay Tall (half is 1020).
        for height in [1000, 1020, 1056] {
            assert_eq!(classify(905, height).height, HeightClass::Tall);
        }
    }

    #[test]
    fn the_queue_page_does_not_also_show_the_sidebar_card() {
        // He caught this in a full screen: the Next Up page shows the queue in the
        // content area, so the sidebar card has to go even in a window that could
        // fit it, and Radio duplicates it the same way. Everything else keeps it.
        let tall = classify(1896, 1056);
        assert!(tall.shows_sidebar_card());
        for page in [LibraryPage::NextUp, LibraryPage::Radio] {
            assert!(
                !tall.shows_sidebar_queue_card(page),
                "{page:?} must not show the sidebar card"
            );
        }
        for page in [
            LibraryPage::Tracks,
            LibraryPage::Albums,
            LibraryPage::Artists,
            LibraryPage::Playlists,
        ] {
            assert!(
                tall.shows_sidebar_queue_card(page),
                "{page:?} keeps the sidebar card"
            );
        }
        // The tier still wins on its own axis, whatever the page.
        let short = classify(941, 503);
        assert!(!short.shows_sidebar_queue_card(LibraryPage::Tracks));
    }

    #[test]
    fn the_queue_has_exactly_one_way_in() {
        // Eddie's rule: the Next Up nav row is visible only while the sidebar's
        // Next Up pill is hidden, so a tall window never shows both.
        for (width, height) in [(905, 656), (941, 503), (466, 503), (905, 951)] {
            let class = classify(width, height);
            assert!(!class.shows_sidebar_card(), "pill {width}x{height}");
            assert!(class.shows_next_up_nav_row(), "row {width}x{height}");
        }
        for (width, height) in [(1896, 1056), (941, 1020)] {
            let class = classify(width, height);
            assert!(class.shows_sidebar_card(), "pill {width}x{height}");
            assert!(!class.shows_next_up_nav_row(), "row {width}x{height}");
        }
    }

    #[test]
    fn the_icon_rail_never_shows_cover_art() {
        // 637x1056 is the size he caught: narrow (icon rail) but tall, so the
        // 220px art was squeezed into a one-column sidebar and pushed the nav out
        // of shape. The rail tiers therefore drop the art, and the widest tile
        // that still has a real sidebar (862x1056, compact) keeps it.
        for width in [519, 637, 779] {
            let class = classify(width, 1056);
            assert!(class.uses_icon_rail(), "{width} should use the rail");
            assert!(!class.shows_cover_art(), "{width}x1056 must not show art");
        }
        for width in [780, 862, 899] {
            let class = classify(width, 1056);
            assert!(
                !class.uses_icon_rail(),
                "{width} should keep the real sidebar"
            );
            assert!(class.shows_cover_art(), "{width}x1056 should keep art");
        }
    }

    #[test]
    fn thresholds_are_ordered_and_exclusive() {
        assert_eq!(classify(WIDTH_COMPACT_BELOW, 900).width, WidthClass::Full);
        assert_eq!(
            classify(WIDTH_COMPACT_BELOW - 1, 900).width,
            WidthClass::Compact
        );
        assert_eq!(
            classify(WIDTH_NARROW_BELOW - 1, 900).width,
            WidthClass::Narrow
        );
        assert_eq!(classify(WIDTH_TINY_BELOW - 1, 900).width, WidthClass::Tiny);
        assert_eq!(classify(900, HEIGHT_SHORT_BELOW).height, HeightClass::Tall);
        assert_eq!(
            classify(900, HEIGHT_SHORT_BELOW - 1).height,
            HeightClass::Short
        );
        assert_eq!(
            classify(900, HEIGHT_TINY_BELOW - 1).height,
            HeightClass::Tiny
        );
    }

    #[test]
    fn css_classes_name_both_axes() {
        assert_eq!(
            classify(466, 503).css_classes(),
            vec!["app-root", "layout-tiny", "height-short"]
        );
        assert_eq!(
            classify(1896, 1020).css_classes(),
            vec!["app-root", "layout-full", "height-tall"]
        );
    }
}
