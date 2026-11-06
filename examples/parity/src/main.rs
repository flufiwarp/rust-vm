use scratch_vm::{CliFrontend, Project, Runtime};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::from_slice(include_bytes!("../project.json"))?;
    let mut runtime = Runtime::new(Project::load_json(&json)?);
    let mut frontend = CliFrontend::new();
    frontend.headless = true;
    // This suite includes 200,001-iteration list tests. Simulate Scratch frames
    // without spending an hour waiting for actual screen refreshes.
    let mut real_time = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--real-time" => real_time = true,
            "--quiet-logs" => runtime.config.quiet_logs = true,
            _ => {
                return Err(format!(
                    "unknown argument {argument:?}; usage: parity [--real-time] [--quiet-logs]"
                )
                .into());
            }
        }
    }
    if !runtime.config.quiet_logs {
        eprintln!(
            "[parity] headless run: collision/input defaults are logged; renderer-dependent checks are not full graphical parity"
        );
    }
    if !real_time {
        frontend.frame_interval = Duration::ZERO;
        frontend.frame_delta = Some(Duration::from_secs_f64(1.0 / 30.0));
    }
    frontend.run_green_flag(&mut runtime).await?;
    let stage = runtime
        .project
        .targets
        .iter()
        .find(|(_, target)| matches!(target, scratch_vm::Target::Stage { .. }))
        .map(|(id, _)| id);
    if let Some(data) = stage.and_then(|id| runtime.scratch.targets.get(id)) {
        let total = data
            .variables
            .values()
            .find(|variable| variable.name == "totalTests")
            .map(|variable| variable.value.to_string())
            .unwrap_or_default();
        let passed = data
            .variables
            .values()
            .find(|variable| variable.name == "passedTests")
            .map(|variable| variable.value.to_string())
            .unwrap_or_default();
        println!(
            "[parity] finished: {passed}/{total} checks passed; {} explicit headless fallbacks",
            frontend.headless_fallbacks
        );
    }
    Ok(())
}
