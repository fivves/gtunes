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
