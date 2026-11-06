use super::*;
use crate::CliFrontend;

#[test]
fn parity_project_reaches_final_results_within_a_bounded_number_of_frames() {
    let json =
        serde_json::from_slice(include_bytes!("../../../examples/parity/project.json")).unwrap();
    let mut runtime = Runtime::new(Project::load_json(&json).unwrap());
    let mut frontend = CliFrontend::with_io(
        std::io::Cursor::new(Vec::new()),
        std::io::sink(),
        std::io::sink(),
    );
    frontend.headless = true;
    frontend.frame_delta = Some(Duration::from_secs_f64(1.0 / 30.0));
    ready(runtime.green_flag()).unwrap();
    let mut frames = 0;
    while !runtime.is_idle() && frames < 250_000 {
        ready(frontend.tick(&mut runtime)).unwrap();
        frames += 1;
    }
    assert!(
        runtime.is_idle(),
        "parity did not terminate within {frames} simulated frames"
    );
    let stage = runtime.stage_id().unwrap();
    let data = &runtime.scratch.targets[&stage];
    let total = data
        .variables
        .values()
        .find(|variable| variable.name == "totalTests")
        .unwrap()
        .value
        .as_number();
    assert!(
        total >= 2_000.0,
        "stopped before running the later groups: only {total} checks"
    );
    let output = data
        .lists
        .values()
        .find(|list| list.name == "output")
        .unwrap();
    assert!(
        output
            .items
            .iter()
            .any(|item| item.to_string().contains("Tests Were")),
        "finalResults was never called"
    );
    assert!(frontend.headless_fallbacks > 0);
    println!(
        "parity completed {total} checks in {frames} simulated frames (headless; completion is not full conformance)"
    );
}
