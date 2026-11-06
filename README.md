# Scratch VM

Compile Scratch project JSON and run the resulting `Instruction` trees using a
cooperative, tick-driven runtime. Enable the `scratch` feature for Scratch blocks,
state, effects, and requests (`mistwarp` also enables `scratch`). Without it, the
base runtime still supports literals, yielding, forever loops, and external requests.

## Running scripts

```rust
let project = scratch_vm::Project::load_json(&project_json)?;
let mut vm = scratch_vm::Runtime::new(project);
vm.green_flag().await?; // Scratch feature

// Once per frontend frame:
vm.tick().await?;
for effect in vm.drain_effects() {
    // Apply effects / start requests in your frontend.
}
```

`run_instructions(&target_id, instructions)` schedules an instruction stack and
returns its thread ID. `run_block`, `run_hat`, and `run_target_hats` start compiled
blocks/scripts. The frontend dispatches keyboard events through `key_pressed`,
click events through `run_target_hats`, and messages through `broadcast_message`.
Other externally detected hat events can also use `run_target_hats`.

`tick()` uses elapsed wall-clock time. `tick_with_delta(Duration)` instead advances
by an explicit amount, useful for deterministic tests and game-loop clocks. These
methods **never await frontend input**. A waiting/sleeping script yields while
other scripts keep running. New scripts begin on the next tick. The configurable
`tasks_per_tick` budget also bounds warp scripts and empty infinite loops.

### Missing custom procedures

Missing procedure definitions **warn and skip the call by default**, allowing the
rest of the script to continue. Each call emits `Effect::Warning` with target and
thread IDs; the CLI prints it to stderr. For strict checking:

```rust
vm.config.error_on_missing_procedures = true;
// Or Runtime::with_config(project, RuntimeConfig {
//     error_on_missing_procedures: true,
//     ..RuntimeConfig::default()
// });
```

Strict mode still emits the warning, then returns `ProcedureNotFound` and finishes
that thread. This setting requires `scratch` and does not suppress malformed
procedure metadata or unrelated runtime errors.

### Scratch Addons debugger

Enable `scratch-addons-debugger` (which automatically enables `scratch`) to handle
Scratch Addons' zero-width-character custom procedure IDs. Both example frontends
enable this feature. Ordinary user procedures named `log`, etc. are not intercepted.

- **log / warn / error:** emit `Effect::Debugger` with a typed `DebuggerEvent::Log`
  and `DebuggerLogLevel`. The CLI prints `[debugger:log]`, `[debugger:warn]`, or
  `[debugger:error]`. The error block logs a message; it does not abort the script.
- **start timer / stop timer:** produce labeled timer events using runtime time.
  Starting an existing label restarts it; stopping an inactive label is a no-op.
  Timers are shared across targets and cleared by the green flag. These events do
  not implement Scratch Addons' browser profiling UI or its optional timing settings.
- **breakpoint / legacy `sa-pause`:** pause all scripts and freeze runtime time,
  emitting a `DebuggerEvent::Breakpoint` and `Request::DebuggerPause`. A frontend
  resumes with `Response::Completed` or `vm.resume_debugger()`. The CLI prompts for
  Enter on its input worker. Cancelling the pause request also unpauses the VM.

`vm.is_debugger_paused()` reports the pause state. With the feature disabled,
debugger procedures follow the same warn/skip (or strict error) policy as any
other missing custom procedure.

## Requests and responses

A blocking frontend interaction emits exactly one `Effect::Request`, containing
`id`, `target_id`, `thread_id`, and the request payload. Keep its ID and respond
when your UI or external operation completes:

```rust
match effect {
    scratch_vm::Effect::Request {
        id, request: scratch_vm::Request::Ask { question }, ..
    } => {
        // Show `question`. Later, in the UI's submit handler:
        vm.respond(id, scratch_vm::Response::Answer(user_text))?;
    }
    scratch_vm::Effect::CancelRequest { id } => {
        // Dismiss the prompt / cancel the corresponding external operation.
    }
    _ => {}
}
```

The thread polls its saved request on each tick and resumes once a response is
available. The Scratch `answer` reporter is updated before the script continues.
Nested asynchronous reporters preserve already evaluated inputs; they do not
restart the enclosing instruction or re-send requests.

| Request | Response |
| --- | --- |
| `External` (base runtime) | `Value` or `Completed` |
| `Ask` | `Answer(String)` |
| `PlaySoundUntilDone`, `Speak` | `Completed` |
| `Sense` | `Value` (the sensor's number, boolean, or string) |
| `Translate` | `Value(Value::String(...))` |
| `Position` (e.g. mouse destination) | `Position { x, y }` |
| `Bounce` (frontend costume bounds) | `Motion { x, y, direction }` |
| `DebuggerPause` (`scratch-addons-debugger`) | `Completed` |

Every request also accepts `Failed(message)`, which finishes its owning thread
and returns a `RuntimeError` from the next tick. Other runnable threads still get
their turn that tick. Invalid response types and duplicate responses return errors
without losing the pending request. Responses to consumed/cancelled IDs return
`UnknownRequest`; frontends should ignore late callbacks from cancelled work.

`pending_request(id)` and `pending_requests()` expose outstanding work for inspection;
do not re-send it each frame. `stop_thread` and `stop_all` cancel pending work.

## Hidden and legacy Scratch blocks

These are loaded, compiled, and executed behind the same `scratch` feature:

- `control_for_each`: assigns the selected variable `1`, `2`, ... while its private
  index is below `VALUE`. The bound is reevaluated each iteration, including
  asynchronous reporters. Like Scratch, a bound of `2.5` runs three iterations.
- `control_while`: reevaluates `CONDITION` and runs its body while true. Both hidden
  loops yield between iterations unless called from a warp custom block.
- Counter (`get`, `increment`, `clear`): shared across targets/clones, initially
  zero. A green-flag restart does not reset it; `control_clear_counter` does.
- `control_all_at_once`: the legacy Scratch 2 wrapper runs its body normally;
  it is **not** a warp custom block.
- `sensing_loud`: requests `Sensor::Loudness`, then reports whether the response is
  strictly greater than 10. No additional frontend sensor is needed.
- `event_whentouchingobject`: retains its touching-object input and can be
  dispatched through `run_hat`/`run_target_hats`, like other frontend-driven hats.
  Collision detection and rising-edge detection remain frontend responsibilities.
- Legacy pen hue/shade blocks emit `PenCommand::{SetHue, ChangeHue, SetShade,
  ChangeShade}`. The frontend must apply Scratch 2's hue/shade semantics rather
  than treating these as modern color-parameter values.
- Hidden MIDI music blocks map MIDI numbers to normal Scratch drum/instrument
  numbers before emitting the existing `MusicCommand` effects.
- Old scroll/alignment/stretch blocks, `looks_hideallsprites`, and `sensing_userid`
  intentionally do nothing, matching Scratch 3's legacy handling. No-op reporters
  return the runtime's empty value. Legacy sound beats/effects menus return their
  selected field values.

## Frontend responsibilities

The VM executes control flow, operators, motion, variable/list scoping, custom
blocks, broadcasts/joins, cloning, waits, glides, and internal Scratch state.
Rendering, audio, pen drawing, external sensors, translation, and speech belong to
the frontend: apply their effects and fulfill their requests. Sound-until-done
and speech requests themselves start the operation; there is no second play effect.

`TargetChanged` includes a transform/state **snapshot**, so intermediate movements
can be used for pen strokes even when many changes happen within one tick. Project
costume/sound metadata is retained on `sprite.scratch`; live variable/list/effect
state is available through `vm.scratch`.

## Optional CLI frontend

Enable `cli-frontend` to use `scratch_vm::CliFrontend` (also available from
`scratch_vm::cli_frontend`). This feature does not enable Scratch implicitly;
enable both features when running Scratch projects. Its async loop uses a Tokio
runtime with time enabled.

```rust
let mut frontend = scratch_vm::CliFrontend::new();
frontend.run_green_flag(&mut vm).await?; // Requires `scratch` too.
// Or run scripts you already scheduled:
// frontend.run(&mut vm).await?;
```

The frontend logs **every effect** to stderr, explicitly marking effects it cannot
handle as `[effect: unsupported]`. Bubbles and Ask prompts go to stdout. Ask input
is read sequentially on a background worker, so other scripts and Tokio tasks keep
running. Unsupported requests receive `Response::Failed` instead of hanging or
pretending to have graphics/audio/sensor data.

To hide diagnostic/effect logs (including warnings, unsupported effects, and
headless fallback notices), while keeping debugger output and say/think text:

```rust
vm.config.quiet_logs = true; // Defaults to false; can be changed at any time.
// Or Runtime::with_config(project, RuntimeConfig {
//     quiet_logs: true,
//     ..RuntimeConfig::default()
// });
```

This setting controls presentation in the shared CLI frontend, not VM execution:
all effects are still emitted/handled, requests still receive responses, and
errors are still returned. Interactive input/breakpoint prompts remain visible.
Custom frontends decide how to use the setting. Example runners also retain
their result summaries and report errors normally. All three examples accept
`--quiet-logs`, for example:

```sh
cargo run --release -p bench -- --quiet-logs
cargo run -p parity -- --quiet-logs
cargo run -p dev -- --quiet-logs
```

Calendar reporters (`current` and `days since 2000`) use the local system clock.
By default collision, keyboard, mouse, and microphone requests remain unsupported.
For explicit headless runs, set `frontend.headless = true`: collision/key/button
queries return false, the mouse position is `(0, 0)`, loudness is `-1` (no mic),
and username is empty (no Scratch login). Each is logged as
`[effect: headless fallback]` and counted in `headless_fallbacks`. These defaults
are not a substitute for a renderer or real input and do not imply full parity.

`Bounce` requests have a headless fallback: the CLI treats the sprite as a point,
reflects outward motion at the 480×360 stage edges, and clamps overshoot back onto
the stage. These requests are logged as `[effect: approximate stage bounds]`.
This keeps `if on edge, bounce` scripts running, but does not account for costume
size, rotation, or rendered collision bounds like a graphical frontend would.

Cancelled questions are skipped when still queued; late replies are ignored. A
read already blocked on stdin cannot be interrupted, so it may still consume one
line after cancellation. Dropping the frontend never waits for a blocked read.

`frame_interval` defaults to 16 ms. Set it to zero to yield to Tokio without
sleeping. `frame_delta` defaults to `None` (wall-clock time); setting a duration
advances the VM by that fixed amount each tick, independent of real elapsed time.
`tick` performs one VM/frontend frame;
`process_effects` just drains effects and polls answers for custom tick loops.
`with_io(input, output, log)` supports alternate console streams and testing.

Both `examples/dev/src/main.rs` and `examples/parity/src/main.rs` use this shared
frontend; neither maintains its own input worker or effect loop. The benchmark
example also uses this frontend.

### Running the parity suite

`parity` defaults to fast, fixed-step, headless execution (30 simulated frames per
second, without actual frame sleeps). Its list-limit test runs **200,001 loop
iterations**; real-time playback would spend roughly an hour there. The project
JSON is not modified or shortened. Use `--real-time` for actual frame sleeps.

The runner prints a final passed/total summary and the headless fallback count.
Completing the suite is not the same as passing all conformance checks: remaining
renderer, coercion, and scheduling differences are still reported as failures.
All effects are logged by default, so use `--quiet-logs` or redirect stderr for
this large suite:

```sh
cargo run -p parity 2>/tmp/parity-effects.log
# Real-time alternative (still headless):
cargo run -p parity -- --real-time
```

There is also a bounded integration regression that requires reaching the
project's own `finalResults` call, not merely becoming idle early:

```sh
cargo test -p scratch-vm --all-features parity_project_reaches_final_results -- --nocapture
```

### Running performance benchmarks

`examples/bench` bundles the Fractch-compiled `performance/project` from
[Mist/scratch-bench](https://git.rotur.dev/Mist/scratch-bench). It runs against the
real clock without frame sleeps, prints the upstream results and `DONE` to stdout,
and logs all effects to stderr. Use a release build and bound execution:

```sh
cargo build --release -p bench
timeout 180s target/release/bench >/tmp/opencode/bench-results.txt 2>/tmp/opencode/bench-effects.log
```

Effect logging overhead is included in the timings, and the log can exceed 2 GB
per run. Use `cargo run --release -p bench -- --quiet-logs` to disable diagnostic
logging while retaining debugger output and benchmark results. Timings with and
without diagnostic logging are not directly comparable. See
[`examples/bench/README.md`](examples/bench/README.md) for source provenance and
instructions to regenerate the `.sb3` and embedded JSON.

```sh
cargo test --workspace --all-features
cargo test -p scratch-vm --no-default-features
cargo test -p scratch-vm --no-default-features --features cli-frontend
cargo run -p dev
cargo run -p parity
```
