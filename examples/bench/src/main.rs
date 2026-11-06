use scratch_vm::{CliFrontend, Project, Runtime, Target};
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::from_slice(include_bytes!("../project.json"))?;
    let mut runtime = Runtime::new(Project::load_json(&json)?);
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--quiet-logs" => runtime.config.quiet_logs = true,
            _ => {
                return Err(
                    format!("unknown argument {argument:?}; usage: bench [--quiet-logs]").into(),
                );
            }
        }
    }
    let mut frontend = CliFrontend::new();
    // Benchmark against the real clock, but don't throttle execution to a
    // display refresh rate. In particular, don't use parity's fixed-step clock.
    frontend.frame_interval = Duration::ZERO;
    let started = Instant::now();
    let result = frontend.run_green_flag(&mut runtime).await;

    let output = runtime
        .project
        .targets
        .iter()
        .find(|(_, target)| matches!(target, Target::Stage { .. }))
        .and_then(|(id, _)| runtime.scratch.targets.get(id))
        .and_then(|data| data.lists.values().find(|list| list.name == "output"))
        .ok_or("benchmark project has no output list")?;
    for line in &output.items {
        println!("{line}");
    }
    result?;
    if output.items.last().map(ToString::to_string).as_deref() != Some("DONE") {
        return Err("benchmark stopped before DONE".into());
    }
    eprintln!(
        "[bench] finished {} benchmarks in {:.2}s (wall clock; diagnostic logging {})",
        output.items.len().saturating_sub(2),
        started.elapsed().as_secs_f64(),
        if runtime.config.quiet_logs {
            "disabled"
        } else {
            "enabled"
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_benchmark_hats_compile() {
        let json = serde_json::from_slice(include_bytes!("../project.json")).unwrap();
        let runtime = Runtime::new(Project::load_json(&json).unwrap());
        assert_eq!(runtime.project.targets.len(), 4);
        let mut compiled = 0;
        for (target_id, target) in &runtime.project.targets {
            for (_, block_id) in &target.sprite().top_levels {
                scratch_vm::compile_block(&runtime.project, block_id, target_id).unwrap();
                compiled += 1;
            }
        }
        assert!(compiled > 0);
    }
}
