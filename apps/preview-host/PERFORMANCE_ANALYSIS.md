# Preview host performance analysis

This report is a static performance analysis of the `preview-host` crate
(`apps/preview-host/src`). It has two goals: confirm that no collection grows
without bound as the host runs, and list the per-frame hot-path allocations and
redundant work that can be trimmed without changing behavior. Every finding
cites the source it is drawn from by file and line so the follow-up fixes can be
made against exact locations.

All line numbers are against the source as it stands when this report was
written. The later "Recommended changes" section is the input to the code-fix
work that follows; it separates the changes that are safe to make from the one
piece of per-frame work that the spec requires and must be left alone.

## Scope and method

This is a static analysis only. The sandbox cannot build, test, or run the
crate: the pinned toolchain (1.99.0) is absent and cannot be downloaded, the
rustup and cargo homes are read-only, and the crates.io registry cache is
read-only so missing dependencies cannot be fetched. Fetching Quake game data is
also not possible, so there is no way to run the host against real assets or to
take a benchmark. No build, test, clippy run, or benchmark was performed for
this report. The findings come from reading the source and tracing the data flow
by hand.

The hot paths examined are the host frame loops and the buffers that cross the
bridge boundary:

- The windowed Host_Frame loop in `src/window.rs`, `Viewer::ui` (one server
  frame of input, simulation, client, sound, and draw per call).
- The headless loops in `src/headless.rs`: the main `run` loop, `timerefresh`,
  and `view_script`.
- The bridge message handling in `src/bridge.rs` (`read_messages` on the reader
  thread, `poll`, and the per-round `serve`).
- The audio mixing and device callback in `src/audio.rs` (`Audio::handle`,
  `Audio::update`, `Queue`, and the `cpal` callback `fill`).
- The screenshot capture path in `src/game.rs` `Game::screenshot_taken` and
  `src/screenshot.rs`.

## Unbounded growth audit

Each accumulating collection is listed with a verdict and the exact site that
drains or bounds it. The short version: every collection is bounded, and the
drains happen once per host frame (for the per-frame buffers) or once per bridge
round. The audio device queue is the model the rest should be read against.

### `Game` buffers (`src/game.rs`)

- `Game.events: Vec<Event>` (declared `src/game.rs:253`). Bounded. Drained every
  bridge round by `take_events` (`src/game.rs:347-349`, `core::mem::take`), which
  `serve` calls once per round (`src/bridge.rs:216-218`). When no client is
  connected the windowed loop still drains and drops it so it cannot build up:
  `serve_bridge` calls `drop(self.game.take_events())` on the no-bridge path
  (`src/window.rs`, `serve_bridge`). Verdict: bounded.
- `Game.responses: Vec<Response>` (declared `src/game.rs:263`). Bounded. Drained
  every bridge round by `take_responses` (`src/game.rs:352-354`), called from
  `serve` (`src/bridge.rs:211-213`). Verdict: bounded.
- `Game.printed: Vec<Vec<u8>>` (declared `src/game.rs:255`). Bounded. In the
  windowed host it is drained each frame by `take_printed`
  (`src/game.rs:811-813`) for the on-screen console; in the headless `run` loop
  it is drained and dropped each frame by `read_messages`
  (`src/headless.rs:417`). Verdict: bounded.
- `Game.shots: Vec<Shot>` (declared `src/game.rs:259`). Bounded. A `screenshot`
  command pushes one entry (`src/game.rs`, `Game::call` screenshot branch), and
  every pushed entry is removed the next frame drawn by `screenshot_taken`
  (`core::mem::take` at `src/game.rs:374`) or by `screenshot_failed`
  (`core::mem::take` at `src/game.rs:391`). Both the windowed loop
  (`src/window.rs`, `wants_screenshot` branch) and the headless loop
  (`src/headless.rs:67-73`) call one of these every frame a screenshot is
  pending. Verdict: bounded.
- `Game.held: Vec<Held>` (declared `src/game.rs:252`). Bounded. Edits are held
  only while the game is paused and are drained in order by `apply_held`
  (`core::mem::take` at `src/game.rs:759`) when the game resumes. The buffer can
  only grow while paused, which is a client-driven, finite state, and it is
  emptied on resume. Verdict: bounded.
- `Game.breakpoints: Vec<Breakpoint>` (declared `src/game.rs:271`). Bounded by
  client action, not by frames. `breakpoint/set` pushes one
  (`src/game/debug.rs:120-125`) and `breakpoint/clear` removes one
  (`src/game/debug.rs:143`). It grows only on an explicit set and shrinks on an
  explicit clear, so it tracks the number of breakpoints the client has set, not
  elapsed time. Verdict: bounded (client-sized, not frame-sized).
- `Game.snapshot: Option<Snapshot>` and `Game.halted: Option<Stopped>`
  (declared `src/game.rs:274` and `src/game.rs:276`). Bounded. Both are
  single-slot `Option`s. `snapshot` is set in `run_frame`
  (`src/game/debug.rs:160-163`) and cleared in `finish_frame_of`
  (`src/game/debug.rs:193`); `halted` is set in `stopped_at` and cleared in
  `finish_frame_of` (`src/game/debug.rs:192`). A single `Option` cannot
  accumulate. Verdict: bounded. (The cost of `snapshot` is the clone it holds, an
  optimization-section item, not a growth item.)
- `Game.lines: Vec<i32>` (declared `src/game.rs:269`). Bounded. Set wholesale by
  `set_lines` (`src/game.rs`) and replaced, not appended, on reload. Its size is
  the progs statement count, not a function of running time. Verdict: bounded.

### Audio buffers (`src/audio.rs`)

- Audio device queue, `Queue.samples: Arc<Mutex<VecDeque<i16>>>` (struct at
  `src/audio.rs:65`). Bounded, and the exemplar. `Output::write` for `Queue`
  caps the queue at `self.most` samples: it computes
  `let room = self.most.saturating_sub(samples.len());` and then
  `samples.extend(frames.iter().take(room));` (`src/audio.rs:71-74`), so it only
  ever admits as many samples as fit under the cap and silently drops the
  overflow. `most` is `MOST_QUEUED` (0.25 seconds) of samples computed by
  `seconds_of_samples` in `device` (`src/audio.rs:392`), with `MOST_QUEUED`
  defined at `src/audio.rs:24`. The device callback `fill` drains frames out of
  the same queue (`src/audio.rs:436-446`). Because a backed-up device cannot make
  the writer wait, this saturating cap is exactly what keeps a stall from
  building unbounded latency. This is the pattern other buffers should be read
  against. Verdict: bounded, correctly.
- Audio capture buffer, `Captured(Vec<i16>)` (struct at `src/audio.rs:54`). Its
  `Output::write` appends every frame with `self.0.extend_from_slice(frames)`
  (`src/audio.rs:57-59`) and, unlike `Queue`, does not cap. It is bounded only
  because it exists solely under `Device::Capture`, which is the `--view-script`
  path, and it is drained each step by `take_captured`
  (`core::mem::take(&mut captured.0)` at `src/audio.rs:351`). In `view_script`
  the drained samples are accumulated into the local `sound` vector
  (`src/headless.rs:184` and `src/headless.rs:236`) and written out once at the
  end of the script (`src/headless.rs:238-239`). That `sound` vector grows for
  the length of the script, which is a finite, offline run with no live device,
  so it is bounded by the script length rather than unbounded over time. Under
  `Device::Default` or `Device::None` there is no `Captured` at all. Verdict:
  bounded (capture mode is offline and drained each step; `sound` is run-length
  bounded, not time-unbounded).
- Audio precache, `Audio.precache: Vec<Option<SfxId>>` (declared
  `src/audio.rs:83`). Bounded. It starts empty (`src/audio.rs:213`) and is
  replaced wholesale, not appended, on every `Event::NewMap`:
  `self.precache = sounds.iter().map(...).collect()` (`src/audio.rs:227-230`).
  Its size is one map's sound precache, and a new map overwrites it. Verdict:
  bounded.

### Bridge channel and reader thread (`src/bridge.rs`)

- The reader-thread to game-loop channel, the `std::sync::mpsc` created in
  `Bridge::over` (`src/bridge.rs:44`). Bounded in steady state. The reader thread
  `read_messages` pushes one `Incoming` per frame it reads
  (`src/bridge.rs:177-194`) and stops the moment it reads `Closed` or the send
  fails (`src/bridge.rs:191-193`). The game loop drains the channel fully each
  round: `poll` loops on `try_recv` until it returns `Empty`
  (`src/bridge.rs:86-96`), and `serve` calls `poll` once per round
  (`src/bridge.rs:208`). Because the channel is emptied every round, it holds
  only the messages that arrived since the last round, which is bounded by the
  client's send rate between two frames, not by elapsed time. Verdict: bounded in
  normal operation. Note: `channel()` is unbounded in principle, so a client that
  floods messages faster than the host drains rounds could grow it; this is a
  robustness observation, not a leak the host causes, and is called out again
  under "Recommended changes" as deliberately left as is.

### Headless and window loops

- Headless `run` (`src/headless.rs:39`): drains `printed` and `messages` each
  frame through `read_messages` (`src/headless.rs:417-418`) and answers the
  bridge each round through `serve` (`src/headless.rs:51`). No per-frame buffer is
  left undrained.
- Windowed `Viewer::ui` (`src/window.rs`): drains the server's messages each
  frame with `take_messages` (`src/window.rs`), serves the bridge twice per
  frame, and drains events, responses, and printed through those calls. No
  per-frame buffer is left undrained.

Audit conclusion: there is no unbounded memory growth. Every accumulating
collection has a drain or a wholesale replacement that runs at least once per
host frame or per bridge round, except the client-sized `breakpoints` and the
offline capture buffer, both of which are bounded by client or script size
rather than by time.

## Hot-path allocation and redundant work

These are optimization opportunities, not growth bugs. Each has a severity and a
note on whether it runs every frame. They are grouped into "safe to change" and
"required behavior, leave as is".

### Safe to change

- Input events cloned twice per frame (window). Severity: medium, per-frame.
  `Viewer::type_keys` reads the egui input and clones the whole event vector:
  `let events = ui.input(|input| input.events.clone());` (`src/window.rs:735`).
  `Viewer::bind_keys` does the same a second time:
  `let (events, modifiers) = ui.input(|input| (input.events.clone(), input.modifiers));`
  (`src/window.rs:623`). Both run every frame in `Viewer::ui`, so the full egui
  event vector is cloned twice per frame even on frames with no input of
  interest. The clone exists to end the `ui.input` borrow before the loop that
  follows; it can be reduced to one clone shared by both passes, or to copying
  out only the key, pointer-button, and wheel events each pass actually consumes.
- `LocalSound` allocates a `String` per event (audio). Severity: low, per-event.
  In `Audio::handle`, the `Event::LocalSound` branch resolves the sound name and
  immediately owns it: `.map(str::to_owned)` (`src/audio.rs:236-245`, the
  `to_owned` at `src/audio.rs:240`) before passing `&name` to
  `self.mixer.local_sound`. The owned `String` is created only to end the borrow
  of `self.table` so `self.mixer` can be borrowed mutably. It allocates once per
  `LocalSound` event, which is infrequent (menu-style local sounds), so the
  severity is low. On closer inspection this allocation turns out to be required
  by the borrow checker and was left as is; see item 2 under "Recommended
  changes" for why no restructuring removes it without changing behavior or
  breaking a public `qs-sound` API.
- `screenshot_taken` clones `frame.rgba` per waiting shot (game). Severity: low,
  not per-frame. `Game::screenshot_taken` builds a `Frame` copy for each waiting
  shot with `rgba: frame.rgba.clone()` (`src/game.rs:378`). This only runs when a
  screenshot is pending (guarded by `wants_screenshot` in both loops), not every
  frame, and there is usually at most one waiting shot. The clone is needed
  because `encode` consumes the `Frame`; when there is exactly one shot the last
  (or only) iteration could move `frame`'s pixels instead of cloning, saving one
  full-frame copy on the common single-shot path.

### Required behavior, leave as is

- Debugger World snapshot clone (game/debug). Severity: high cost, but
  REQUIRED, per-frame only while a debugger may stop. `run_frame` clones the
  entire `World` before a frame when the debugger may stop it:
  `self.snapshot = Some(Snapshot { world: self.world.clone(), frametime });`
  (`src/game/debug.rs:160-163`), and `replay` clones the snapshot again
  (`src/game/debug.rs:170`) to re-run the frame. This is the most expensive
  per-frame allocation in the crate, but it is required by WORLD-9.2: `step` and
  `resume` replay the stopped frame from the pre-frame world, so the pre-frame
  world must be captured before the frame runs. It only happens while a debugger
  is attached and armed (`Debugger::may_stop`), not during ordinary play. This
  clone must NOT be removed or weakened; doing so would break the replay
  semantics the spec mandates.

### Buffers reused vs reallocated

The bounded buffers are reused rather than freed and reallocated: `core::mem::take`
leaves an empty `Vec`/`VecDeque` in place that keeps its capacity, so
`events`, `responses`, `printed`, `shots`, `held`, and the capture `Vec` all keep
their backing allocation across frames once warmed. The device `Queue` is
constructed once with `VecDeque::with_capacity(most)` (`src/audio.rs:394`) and
never reallocates past its cap. No change is needed here; it is noted so the
fixes do not "optimize" a take into something that drops capacity.

## Recommended changes

The three behavior-preserving changes below were identified by this report. Two
were applied in the follow-up fix work and one was found to be required by the
borrow checker and left as is. Each entry records what was actually done so the
report and the diff agree.

1. Done. The two per-frame `input.events.clone()` calls in `window.rs`
   (`type_keys` and `bind_keys`) were collapsed into a single clone per frame.
   `Viewer::ui` now reads the frame's events and modifiers once in one
   `ui.input(|input| (input.events.clone(), input.modifiers))` and passes the
   borrowed slice to `type_keys(&[egui::Event])` and
   `bind_keys(&[egui::Event], egui::Modifiers)`. The event vector is cloned once
   instead of twice; the two passes see the same event order and the same
   modifier snapshot they saw before, since both reads already read the same
   frame's fixed egui input, so behavior is unchanged. Medium value, runs every
   frame.
2. Left as is (required by the borrow checker). The per-event `String`
   allocation in the `LocalSound` branch of `Audio::handle` cannot be removed
   without changing behavior. The name is obtained from `self.table.name(id)`,
   which borrows `self.table` immutably and returns a `&str` that points into the
   table; the very next call, `self.mixer.local_sound(&mut self.table, rand,
   name)`, needs `self.table` borrowed mutably while that same `&str` is passed
   in. An immutable borrow of `self.table` (the name) therefore cannot be held
   across the `&mut self.table` call, so the name must be owned first with
   `str::to_owned`. The obvious alternatives are not behavior-preserving: the
   mixer's public `local_sound(table, rand, name: &str)` is the only entry point
   that honours the `nosound` early-return and the "can't cache" message path,
   and it re-precaches the name (which can reload the sound's data); calling the
   lower-level `start_sound` with the already-resolved `SfxId` would skip that
   re-precache and would also need the mixer's private `view_entity` field to
   build the `Start`, so it is both a behavior change and an API break. Changing
   `local_sound` to take an `SfxId` would change a public `qs-sound` API and the
   separate menu `Audio::local` caller, which is out of scope for a
   behavior-preserving pass. The allocation is low value anyway: it happens once
   per `LocalSound` event (infrequent, menu-style local sounds), not every frame.
3. Done. On the single-waiting-shot path (the common case),
   `Game::screenshot_taken` now moves `frame.rgba` into `encode` instead of
   cloning it. The method takes `frame: Frame` by value, peels the last waiting
   shot, clones the pixels only for the earlier shots, and moves the pixels into
   the final shot's `encode`. Output is identical for zero shots (nothing is
   answered), one shot (the only shot owns the pixels, no clone), and many shots
   (each earlier shot still gets its own copy). The two call sites
   (`window.rs`, `headless.rs`) now pass the owned `Frame`, and the two
   `screenshot_taken` tests were updated to pass by value. Low value, only when a
   screenshot is pending.

Deliberately left unchanged:

- The debugger World snapshot clone in `src/game/debug.rs` `run_frame`
  (`src/game/debug.rs:160-163`) and the snapshot clone in `replay`
  (`src/game/debug.rs:170`). Required by WORLD-9.2 replay semantics: the frame
  must be replayable from the pre-frame world, so the world is snapshotted before
  the frame. Do not remove.
- The audio device `Queue` cap in `Output::write` for `Queue`
  (`src/audio.rs:71-74`). It is already the correct bounded-buffer pattern and
  should be left as the exemplar.
- The unbounded `std::sync::mpsc::channel()` in `Bridge::over`
  (`src/bridge.rs:44`). The host drains it fully every round
  (`src/bridge.rs:86-96`), so it is bounded in normal operation; converting it to
  a bounded channel would change backpressure behavior and is out of scope for a
  behavior-preserving pass.

## Verification

Verification for this report is static, per the sandbox constraints stated under
"Scope and method": the toolchain and registry are read-only and the pinned
toolchain is absent, so the crate cannot be built, tested, linted, or
benchmarked here. The report was verified by reading the cited source and
confirming each file and line reference against the current `apps/preview-host/src`
tree. No build or benchmark was run, and none is claimed.
