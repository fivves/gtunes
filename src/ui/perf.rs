//! Env-gated UI timing for the collection grids.
//!
//! `GTUNES_UI_PERF=1` records the frame clock's own cadence and prints a line a
//! second, so a scroll can be described in numbers instead of "feels janky".
//! `GTUNES_PERF_SCROLL=albums|artists|playlists` additionally drives a timed
//! scroll sweep through that grid, which is the same draw path a wheel uses but
//! repeatable from a script. Nothing here runs unless the variables are set.

use super::prelude::*;

/// Grid frames slower than this are the ones a person notices.
const SLOW_FRAME_MS: f64 = 20.0;
/// Where the sweep starts scrolling, and how far it travels per frame.
const SWEEP_START_DELAY_MS: u64 = 2_500;
const SWEEP_STEP_PX: f64 = 14.0;
const SWEEP_SECONDS: u64 = 10;

pub(crate) fn install_perf_hooks(root: &gtk::Box, state: Rc<RefCell<UiState>>) {
    if std::env::var_os("GTUNES_UI_PERF").is_none() {
        return;
    }

    let samples: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));
    let last_frame: Rc<RefCell<Option<Instant>>> = Rc::new(RefCell::new(None));

    {
        let samples = samples.clone();
        let last_frame = last_frame.clone();
        root.add_tick_callback(move |_, _clock| {
            let now = Instant::now();
            if let Some(previous) = *last_frame.borrow() {
                samples
                    .borrow_mut()
                    .push(now.duration_since(previous).as_secs_f64() * 1000.0);
            }
            *last_frame.borrow_mut() = Some(now);
            gtk::glib::ControlFlow::Continue
        });
    }

    {
        let samples = samples.clone();
        gtk::glib::timeout_add_local(Duration::from_secs(1), move || {
            let mut samples = samples.borrow_mut();
            if samples.is_empty() {
                return gtk::glib::ControlFlow::Continue;
            }
            let mut sorted = samples.clone();
            samples.clear();
            sorted.sort_by(f64::total_cmp);
            let percentile = |p: f64| {
                let index = ((sorted.len() as f64 - 1.0) * p).round() as usize;
                sorted[index]
            };
            let slow = sorted.iter().filter(|ms| **ms > SLOW_FRAME_MS).count();
            println!(
                "perf: frames={:>3} p50={:5.1}ms p95={:5.1}ms max={:6.1}ms slow={:>3} decodes={} decode_ms={:.0} tiles={}",
                sorted.len(),
                percentile(0.50),
                percentile(0.95),
                sorted.last().copied().unwrap_or_default(),
                slow,
                artwork_decode_count(),
                artwork_decode_millis(),
                collection_tiles_built(),
            );
            gtk::glib::ControlFlow::Continue
        });
    }

    if std::env::var("GTUNES_PERF_CYCLE").is_ok() {
        install_page_cycle(state);
        return;
    }

    let Some(page) = scroll_target_page() else {
        return;
    };
    gtk::glib::timeout_add_local_once(Duration::from_millis(SWEEP_START_DELAY_MS), move || {
        state.borrow_mut().search_query.clear();
        set_library_page(&state, page);
        let grid = active_collection_grid(&state);
        let Some(grid) = grid else {
            println!("perf: sweep target has no grid");
            return;
        };
        let Some(scroll_page) = collection_grid_scroll(&grid) else {
            println!("perf: sweep target has no scrolled window");
            return;
        };
        let adjustment = scroll_page.vadjustment();
        println!(
            "perf: sweep start page={page:?} tiles={} upper={:.0}",
            grid.observe_children().n_items(),
            adjustment.upper()
        );
        adjustment.set_value(0.0);

        let started = Instant::now();
        let step = std::cell::Cell::new(0.0f64);
        scroll_page.add_tick_callback(move |_, _clock| {
            let elapsed = started.elapsed();
            if elapsed > Duration::from_secs(SWEEP_SECONDS) {
                println!("perf: sweep done after {:.1}s", elapsed.as_secs_f64());
                return gtk::glib::ControlFlow::Break;
            }
            let upper = adjustment.upper().max(1.0);
            let page_size = adjustment.page_size();
            let limit = (upper - page_size).max(0.0);
            let mut offset = step.get() + SWEEP_STEP_PX;
            if offset > limit {
                // Jump back so the sweep keeps producing damage for its full run.
                offset = 0.0;
            }
            step.set(offset);
            adjustment.set_value(offset);
            gtk::glib::ControlFlow::Continue
        });
    });
}

/// `GTUNES_UI_PERF_SKIP=layout,playback` leaves a timer out so its cost can be
/// separated from everything else while measuring.
pub(crate) fn perf_skip(which: &str) -> bool {
    std::env::var("GTUNES_UI_PERF_SKIP")
        .map(|value| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(which))
        })
        .unwrap_or(false)
}

/// `GTUNES_PERF_CYCLE=1` walks Albums -> Artists -> Tracks -> Albums ... on a
/// timer, which is the "open a grid again" case: the tiles counter shows whether
/// the page paid for a rebuild or reused what was already there.
fn install_page_cycle(state: Rc<RefCell<UiState>>) {
    const PAGES: [LibraryPage; 3] = [
        LibraryPage::Albums,
        LibraryPage::Artists,
        LibraryPage::Tracks,
    ];
    let index = std::cell::Cell::new(0usize);
    gtk::glib::timeout_add_local(Duration::from_millis(2_500), move || {
        let page = PAGES[index.get() % PAGES.len()];
        index.set(index.get() + 1);
        println!("perf: cycle -> {page:?} tiles={}", collection_tiles_built());
        set_library_page(&state, page);
        gtk::glib::ControlFlow::Continue
    });
}

fn scroll_target_page() -> Option<LibraryPage> {
    let value = std::env::var("GTUNES_PERF_SCROLL").ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "albums" => Some(LibraryPage::Albums),
        "artists" => Some(LibraryPage::Artists),
        "playlists" => Some(LibraryPage::Playlists),
        _ => None,
    }
}
