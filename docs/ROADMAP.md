# Roadmap

This roadmap focuses on making playback behavior predictable and easier to
maintain. The app should prefer visible failures over surprising restarts, and
background integrations should not implicitly drive playback state.

## Current Observations

1. `src/ui/window.rs` is doing too much.
   - It currently acts as the app controller, view builder, playback
     coordinator, queue manager, MPRIS bridge, Discord bridge, cache restore
     path, waveform UI, radio UI, and test host.
   - Playback behavior should be easier to reason about if session state and
     external integrations move out into focused modules.

2. Discord and MPRIS updates are coupled.
   - `update_mpris_status` and `update_mpris_metadata` also update Discord Rich
     Presence.
   - MPRIS updates should not implicitly control Discord behavior.

3. Playback recovery is too eager.
   - Direct-play fallback still restarts from the beginning when a real
     GStreamer error triggers transcoding.
   - Recovery should preserve the current position when possible.

4. Playback truth is split between `PlaybackEngine` and `UiState`.
   - `PlaybackEngine` tracks low-level item, stream kind, and GStreamer state.
   - `UiState` separately tracks now playing, queue order, selected index, radio
     mode, and visible playback state.
   - A dedicated playback session abstraction would reduce drift between those
     pieces of state.

5. Background work can touch persistence during playback.
   - Discord artwork upload persists URLs from a worker thread.
   - This is not necessarily broken, but it is worth watching for SQLite locking
     or hidden coupling with foreground cache work.

6. The app has several timer loops.
   - Timers are used for playback UI updates, MPRIS polling, artwork loading,
     waveform work, and delayed UI tasks.
   - Timer-driven code should avoid destructive playback decisions.

7. Gapless transition logic is clever but fragile.
   - The GStreamer `about-to-finish` signal swaps the URI and records a pending
     transition.
   - The UI timer later applies that transition to visible state.
   - Queue or now-playing bugs around track boundaries should inspect this path
     first.

8. Tests are useful but mostly pure-state tests.
   - Existing tests cover cache, sorting, queue helpers, request construction,
     and parsing well.
   - Playback behavior would benefit from more pure session-state tests once
     playback state is extracted from the UI module.

## Near-Term Priorities

1. [x] Decouple Discord from MPRIS updates.
   - Stop updating Discord Rich Presence from `update_mpris_status` and
     `update_mpris_metadata`.
   - Introduce an explicit presence sync path for external integrations.
   - Keep Discord and MPRIS failures isolated from core playback behavior.
   - Completed by keeping MPRIS updates inside `update_mpris_status` and
     `update_mpris_metadata`, with Discord handled through explicit external
     playback sync helpers.

2. [ ] Extract playback session state out of `src/ui/window.rs`.
   - Move queue, current item, stream kind, radio mode, and fallback state into a
     dedicated playback/session module.
   - Keep the UI responsible for rendering state, not owning playback truth.
   - Add focused tests for session transitions once the state is isolated.
   - Progress: playback order construction, upcoming queue indexing, drag
     reordering, and queue-next mutation now live in `src/playback/session.rs`
     with focused tests.
   - Progress: persisted playback restore now delegates saved item ID
     deduping, missing-track filtering, current-item lookup, and order rebuild
     to `src/playback/session.rs`.
   - Progress: persisted playback snapshot data and ordered item ID
     construction now live in `src/playback/session.rs`; `window.rs` only
     gathers UI/runtime inputs and handles cache storage.
   - Progress: radio/library playback mode now lives in
     `src/playback/session.rs` through `PlaybackMode`; `window.rs` no longer
     owns a raw radio-station ID field.
   - Progress: direct-play fallback eligibility and fallback status messaging
     now live in `src/playback/session.rs`; `window.rs` performs track lookup
     and GStreamer operations, then delegates fallback bookkeeping.
   - Progress: gapless transition index selection now lives in
     `src/playback/session.rs`; `window.rs` applies the selected index to UI
     state after the playback engine reports a transition.
   - Progress: queue tracks, current queue index, playback order,
     now-playing key, shuffle state, and playback mode are consolidated under
     `session::PlaybackSession<UiTrack>` instead of separate `UiState` fields.
   - Progress: playback order rebuilding, current-index fallback,
     next/previous lookup, upcoming counts, upcoming reordering, and queue-next
     mutation are now methods on `session::PlaybackSession`; `window.rs`
     delegates these state transitions instead of manipulating order state
     directly.
   - Progress: library-track playback selection now delegates queue
     initialization, queue index selection, playback order rebuilds, and
     visible-row mapping to `session::PlaybackSession`.
   - Progress: queue-next setup now lives in `session::PlaybackSession`,
     including radio-mode rejection, queue initialization, stale-order rebuild,
     missing-track append, and next-track ordering.
   - Progress: persisted playback restore application now lives in
     `session::PlaybackSession`, including queue state, current index, playback
     order, shuffle state, now-playing key, library mode, and visible-row
     selection.
   - Progress: library-refresh playback reconciliation now lives in
     `session::PlaybackSession`, including refreshed queued track replacement
     and stale now-playing/queue clearing.
   - Progress: shuffle toggling now lives in `session::PlaybackSession`,
     including flag changes and playback-order rebuilds from either the active
     queue or visible library count.
   - Progress: radio activation now lives in `session::PlaybackSession`,
     including station mode selection, now-playing clearing, and queue reset.
   - Progress: library playback start and now-playing clearing now live in
     `session::PlaybackSession`; playback, fallback, gapless, and end/error
     paths no longer assign the current item key directly.
   - Progress: gapless transition application now lives in
     `session::PlaybackSession`; `window.rs` no longer assigns the active
     queue index directly when the playback engine reports a transition.
   - Progress: playback-finished session cleanup now lives in
     `session::PlaybackSession`; end-of-queue handling clears current playback
     and queue state through a single session transition while preserving mode.
   - Progress: empty-library reset now lives in `session::PlaybackSession`;
     disconnect/cache-reset UI no longer clears shuffle and playback state as
     separate field mutations.

3. [x] Preserve position during direct-play fallback.
   - Capture the current playback position before retrying with a Jellyfin
     transcode stream.
   - Seek the fallback stream back to the captured position when possible.
   - Surface a clear status message if fallback succeeds but seeking fails.
   - Completed by capturing the current GStreamer position before direct-play
     fallback, seeking the transcoded stream to that position, and reporting
     either the restored timestamp or a seek restore failure in playback status.

4. [x] Keep the playback timer non-destructive.
   - Use the timer for UI position, waveform progress, and persistence updates.
   - Do not make restart, fallback, or stop decisions from position polling.
   - Continue relying on GStreamer bus errors and end-of-stream events for
     automatic playback transitions.
   - Completed by removing position-stall detection from the 250ms playback
     timer; fallback now flows through GStreamer error events, and position
     polling only updates UI progress and snapshots.

5. [ ] Add playback-session tests.
   - Cover queue advancement, shuffle ordering, gapless transition bookkeeping,
     radio isolation, fallback state, and persisted restore behavior.
   - Prefer pure state tests that do not require GTK or a live GStreamer
     pipeline.
   - Progress: `src/playback/session.rs` now has focused tests for playback
     order construction, shuffle start behavior, next/previous navigation,
     queued index limits, upcoming counts, drag reordering, and queue-next
     mutation.
   - Progress: persisted restore tests now cover saved queue deduping,
     missing library items, current item lookup, and restore failure when the
     current item is unavailable.
   - Progress: snapshot tests now cover queue item deduping, persisted version
     assignment, position/shuffle capture, and invalid snapshot rejection.
   - Progress: radio isolation has focused session coverage through
     `PlaybackMode` tests for library mode, radio mode, station IDs, and mode
     reset.
   - Progress: fallback tests now cover direct-only transcode retry
     eligibility, restored-position status, failed seek-restore status, and
     no-position fallback status.
   - Progress: gapless transition tests now cover matching item IDs, fallback
     to the next ordered item, and missing-match/missing-next behavior.
   - Progress: `PlaybackSession` tests now cover default empty library state,
     queue clearing, and reset-to-library behavior.
   - Progress: `PlaybackSession` method tests now cover order rebuild,
     next/previous navigation, queued index limits, upcoming counts,
     rebuild-needed detection, queue-next mutation, and upcoming reorder
     mutation.
   - Progress: `PlaybackSession` tests now cover library-track playback
     selection, existing-queue selection, and empty-library clamping.
   - Progress: `PlaybackSession` tests now cover queue-next setup from empty
     queues, missing queued tracks, radio-mode rejection, and stale-order
     rebuilds.
   - Progress: `PlaybackSession` tests now cover applying restored playback
     state, hidden-current fallback selection, and invalid restore index
     rejection.
   - Progress: `PlaybackSession` tests now cover library-refresh queue
     replacement, disappeared now-playing clearing, and inactive queue clearing.
   - Progress: `PlaybackSession` tests now cover shuffle toggling with queued
     playback and library-count fallback.
   - Progress: `PlaybackSession` tests now cover radio activation clearing
     library playback state while preserving shuffle.
   - Progress: `PlaybackSession` tests now cover library playback start and
     current-item clearing without disturbing queue or mode state.
   - Progress: `PlaybackSession` tests now cover applying gapless transitions
     by matching item ID, falling back to the next ordered item, and preserving
     the current index when no transition target exists.
   - Progress: `PlaybackSession` tests now cover playback-finished cleanup for
     library and radio modes.
   - Progress: `PlaybackSession` tests now cover empty-library reset clearing
     shuffle, mode, current item, and queue state.

## Follow-Up Candidates

- Split `src/ui/window.rs` into smaller modules after playback session state is
  extracted.
- Review Discord artwork cache writes from the worker thread for SQLite locking
  behavior.
- Revisit gapless transition handling and reduce reliance on timer polling.
- Improve playback diagnostics for real GStreamer errors, missing codecs, and
  failed fallback streams.

## Guardrails

- Do not reintroduce position-based automatic restart logic.
- Do not let Discord, MPRIS, artwork loading, waveform generation, or cache work
  control playback implicitly.
- Prefer user-visible error states over hidden recovery that restarts the
  current track.
- Keep stream URL construction centralized in the Jellyfin client.
- Route playback changes through `src/playback/` or the future playback session
  module, not directly from view code.
