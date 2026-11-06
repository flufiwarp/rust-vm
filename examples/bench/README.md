# Performance benchmark example

Runs the unmodified `performance/project` from
[Mist/scratch-bench](https://git.rotur.dev/Mist/scratch-bench), commit
`1677fc9972d02f061c9ebcb011b02eda6f306963`, compiled using Fractch 0.7.9.
Upstream is MIT-licensed; see `LICENSE.scratch-bench`.

`project.sb3` is the complete Fractch output. `project.json` is extracted from
that archive and embedded by the Rust example, matching the other examples.

```sh
cargo build --release -p bench
timeout 180s target/release/bench > /tmp/opencode/bench-results.txt 2> /tmp/opencode/bench-effects.log
```

Or use `cargo run --release -p bench` (redirect stderr to avoid flooding the
terminal). Results are printed to stdout after execution, including the upstream
header and `DONE` marker; progress and **all effects** are logged to stderr by the
shared CLI frontend. The runner errors if the project stops without `DONE`.
The effect log can exceed **2 GB per run**; allow disk space or compress it (for
example, `gzip -1 /tmp/opencode/bench-effects.log`).

To disable diagnostic/effect logging without hiding debugger messages or results:

```sh
cargo run --release -p bench -- --quiet-logs
```

This sets `runtime.config.quiet_logs = true`; it doesn't discard VM effects or
suppress returned errors. Debugger output, say/think text, and input prompts are
preserved. Diagnostic logging stays enabled by default.

The suite has 47 benchmarks, using its original 100 ms round budgets and warmup
rounds. Execution uses wall-clock time with no artificial frame sleeps, **not**
parity's simulated clock. Timing includes frontend/effect logging overhead and
is not directly comparable to upstream browser measurements. This is a runtime
benchmark, not a claim of Scratch conformance.
Runs using `--quiet-logs` also aren't directly comparable to fully logged runs.

## Rebuild the bundled project

From the repository root, with Fractch installed:

```sh
git clone https://git.rotur.dev/Mist/scratch-bench.git /tmp/opencode/scratch-bench
git -C /tmp/opencode/scratch-bench checkout 1677fc9972d02f061c9ebcb011b02eda6f306963
fractch to examples/bench/project.sb3 from /tmp/opencode/scratch-bench/performance/project
python3 -c 'import pathlib, zipfile; pathlib.Path("examples/bench/project.json").write_bytes(zipfile.ZipFile("examples/bench/project.sb3").read("project.json"))'
```
