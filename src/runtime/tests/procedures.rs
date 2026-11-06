use super::*;
use std::collections::HashMap;

fn call(code: &str, argument: Option<Instruction>) -> Instruction {
    let arguments = argument
        .map(|value| HashMap::from([("arg0".to_string(), value)]))
        .unwrap_or_default();
    let ids: Vec<_> = arguments.keys().cloned().collect();
    Instruction::ProceduresCall {
        mutation: json!({"proccode": code, "argumentids": serde_json::to_string(&ids).unwrap(), "warp": "false"}),
        arguments,
    }
}

#[test]
fn missing_procedures_warn_and_continue_by_default() {
    let (mut runtime, target) = runtime();
    let thread = runtime
        .run_instructions(
            &target,
            vec![call("missing %s", None), Instruction::IncrCounter],
        )
        .unwrap();
    tick(&mut runtime);
    assert!(runtime.is_idle());
    assert_eq!(runtime.scratch.counter, 1.0);
    assert!(
        matches!(runtime.drain_effects().as_slice(), [Effect::Warning { target_id, thread_id, message }]
        if *target_id == target && *thread_id == thread && message.contains("missing %s") && message.contains("skipping call"))
    );
}

#[test]
fn strict_missing_procedures_warn_before_returning_an_error() {
    let (mut runtime, target) = runtime();
    runtime.config.error_on_missing_procedures = true;
    runtime
        .run_instructions(
            &target,
            vec![call("missing", None), Instruction::IncrCounter],
        )
        .unwrap();
    runtime
        .run_instructions(&target, vec![Instruction::IncrCounter])
        .unwrap();
    assert!(
        matches!(ready(runtime.tick()), Err(RuntimeError::ProcedureNotFound(code)) if code == "missing")
    );
    assert_eq!(runtime.scratch.counter, 1.0); // The other thread still had its turn.
    assert!(runtime.is_idle());
    assert!(
        matches!(runtime.drain_effects().as_slice(), [Effect::Warning { message, .. }] if message.contains("strict mode"))
    );
}

#[test]
fn runtime_config_can_be_supplied_at_construction() {
    let project = Project::load_json(&json!({"targets": []})).unwrap();
    let runtime = Runtime::with_config(
        project,
        RuntimeConfig {
            error_on_missing_procedures: true,
            ..RuntimeConfig::default()
        },
    );
    assert!(runtime.config.error_on_missing_procedures);
}

#[test]
fn malformed_procedure_metadata_is_still_an_error() {
    let (mut runtime, target) = runtime();
    runtime
        .run_instructions(
            &target,
            vec![Instruction::ProceduresCall {
                mutation: json!({}),
                arguments: HashMap::new(),
            }],
        )
        .unwrap();
    assert!(matches!(
        ready(runtime.tick()),
        Err(RuntimeError::InvalidInstruction(_))
    ));
}

#[test]
fn stop_this_script_returns_from_only_the_innermost_procedure() {
    let project = Project::load_json(&json!({"targets": [{"name": "Sprite", "blocks": {
        "outer": {"opcode": "procedures_definition", "inputs": {"custom_block": [1, "outer-prototype"]}, "next": "repeat"},
        "outer-prototype": {"opcode": "procedures_prototype", "mutation": {"proccode": "outer %s", "argumentids": "[\"arg0\"]", "argumentnames": "[\"n\"]", "warp": "true"}},
        "repeat": {"opcode": "control_repeat", "inputs": {"TIMES": [1, [4, 3]], "SUBSTACK": [2, "call-inner"]}, "next": "save"},
        "call-inner": {"opcode": "procedures_call", "mutation": {"proccode": "inner %s", "argumentids": "[\"arg0\"]"}, "inputs": {"arg0": [1, [4, 99]]}, "next": "increment"},
        "increment": {"opcode": "control_incr_counter"},
        "save": {"opcode": "data_setvariableto", "fields": {"VARIABLE": ["saved", "saved"]}, "inputs": {"VALUE": [2, "n"]}, "next": "return-outer"},
        "n": {"opcode": "argument_reporter_string_number", "fields": {"VALUE": ["n", null]}},
        "return-outer": {"opcode": "control_stop", "fields": {"STOP_OPTION": ["this script", null]}, "next": "bad-outer"},
        "bad-outer": {"opcode": "control_incr_counter"},
        "inner": {"opcode": "procedures_definition", "inputs": {"custom_block": [1, "inner-prototype"]}, "next": "forever"},
        "inner-prototype": {"opcode": "procedures_prototype", "mutation": {"proccode": "inner %s", "argumentids": "[\"arg0\"]", "argumentnames": "[\"n\"]", "warp": "false"}},
        "forever": {"opcode": "control_forever", "inputs": {"SUBSTACK": [2, "return-inner"]}, "next": "bad-inner"},
        "return-inner": {"opcode": "control_stop", "fields": {"STOP_OPTION": ["this script", null]}},
        "bad-inner": {"opcode": "control_incr_counter"}
    }}]})).unwrap();
    let target = *project.targets.keys().next().unwrap();
    let mut runtime = Runtime::new(project);
    let mut thread = VmThread::new(
        target,
        vec![
            call("outer %s", Some(Instruction::Literal(Value::Number(42.0)))),
            Instruction::IncrCounter,
        ],
    );
    ready(runtime.run_thread(&mut thread)).unwrap();
    assert_eq!(thread.status, ThreadStatus::Finished);
    assert_eq!(runtime.scratch.counter, 4.0);
    assert_eq!(
        runtime.scratch.targets[&target].variables["saved"].value,
        Value::Number(42.0)
    );
    assert_eq!(thread.warp_depth, 0);
    assert!(thread.arguments.is_empty());
    assert!(thread.values.is_empty());
}

#[test]
fn stop_this_script_outside_a_procedure_still_finishes_the_thread() {
    let (mut runtime, target) = runtime();
    runtime
        .run_instructions(
            &target,
            vec![
                Instruction::Stop {
                    option: crate::Field {
                        value: Value::String("this script".to_string()),
                        id: None,
                    },
                },
                Instruction::IncrCounter,
            ],
        )
        .unwrap();
    tick(&mut runtime);
    assert!(runtime.is_idle());
    assert_eq!(runtime.scratch.counter, 0.0);
}

#[test]
fn repeated_missing_calls_do_not_accumulate_stack_values() {
    let (mut runtime, target) = runtime();
    runtime
        .run_instructions(
            &target,
            vec![Instruction::Repeat {
                times: Box::new(Instruction::Literal(Value::Number(50.0))),
                body: vec![call("missing", None), Instruction::IncrCounter],
            }],
        )
        .unwrap();
    for _ in 0..51 {
        tick(&mut runtime);
    }
    assert!(runtime.is_idle());
    assert_eq!(runtime.scratch.counter, 50.0);
    assert_eq!(runtime.drain_effects().len(), 50);
}

#[cfg(not(feature = "scratch-addons-debugger"))]
#[test]
fn addon_calls_are_missing_procedure_warnings_when_the_feature_is_disabled() {
    let (mut runtime, target) = runtime();
    runtime
        .run_instructions(
            &target,
            vec![call(
                "\u{200b}\u{200b}log\u{200b}\u{200b} %s",
                Some(Instruction::Literal(Value::String("hello".to_string()))),
            )],
        )
        .unwrap();
    tick(&mut runtime);
    assert!(runtime.is_idle());
    assert!(matches!(
        runtime.drain_effects().as_slice(),
        [Effect::Warning { .. }]
    ));
}

#[cfg(feature = "scratch-addons-debugger")]
mod addons {
    use super::*;
    const LOG: &str = "\u{200b}\u{200b}log\u{200b}\u{200b} %s";
    const WARN: &str = "\u{200b}\u{200b}warn\u{200b}\u{200b} %s";
    const ERROR: &str = "\u{200b}\u{200b}error\u{200b}\u{200b} %s";
    const BREAKPOINT: &str = "\u{200b}\u{200b}breakpoint\u{200b}\u{200b}";
    const START: &str = "\u{200b}\u{200b}start timer\u{200b}\u{200b} %s";
    const STOP: &str = "\u{200b}\u{200b}stop timer\u{200b}\u{200b} %s";

    fn value(text: &str) -> Instruction {
        Instruction::Literal(Value::String(text.to_string()))
    }

    #[test]
    fn exported_addon_calls_compile_and_run_without_a_procedure_definition() {
        let project = Project::load_json(&json!({"targets": [{"name": "Sprite", "blocks": {
            "log": {"opcode": "procedures_call", "next": "after", "mutation": {
                "proccode": LOG, "argumentids": "[\"arg0\"]", "argumentnames": "[\"content\"]", "warp": "false"
            }, "inputs": {"arg0": [1, [10, "exported log"]]}},
            "after": {"opcode": "control_incr_counter"}
        }}]})).unwrap();
        let target = *project.targets.keys().next().unwrap();
        let mut runtime = Runtime::new(project);
        ready(runtime.run_block(&target, &"log".to_string())).unwrap();
        tick(&mut runtime);
        assert!(runtime.is_idle());
        assert_eq!(runtime.scratch.counter, 1.0);
        assert!(
            matches!(runtime.drain_effects().as_slice(), [Effect::Debugger { event: DebuggerEvent::Log { message, .. }, .. }] if message == "exported log")
        );
    }

    #[test]
    fn log_warn_and_error_use_exact_addon_signatures_even_in_strict_mode() {
        let (mut runtime, target) = runtime();
        runtime.config.error_on_missing_procedures = true;
        let thread = runtime
            .run_instructions(
                &target,
                vec![
                    call(LOG, Some(value("hello"))),
                    call(WARN, Some(value("warning"))),
                    call(ERROR, Some(value("error"))),
                    Instruction::IncrCounter,
                ],
            )
            .unwrap();
        tick(&mut runtime);
        assert!(runtime.is_idle());
        assert_eq!(runtime.scratch.counter, 1.0);
        assert_eq!(
            runtime.drain_effects(),
            vec![
                Effect::Debugger {
                    target_id: target,
                    thread_id: thread,
                    event: DebuggerEvent::Log {
                        level: DebuggerLogLevel::Log,
                        message: "hello".to_string()
                    }
                },
                Effect::Debugger {
                    target_id: target,
                    thread_id: thread,
                    event: DebuggerEvent::Log {
                        level: DebuggerLogLevel::Warn,
                        message: "warning".to_string()
                    }
                },
                Effect::Debugger {
                    target_id: target,
                    thread_id: thread,
                    event: DebuggerEvent::Log {
                        level: DebuggerLogLevel::Error,
                        message: "error".to_string()
                    }
                },
            ]
        );
    }

    #[test]
    fn addon_argument_ids_are_respected_and_async_inputs_are_not_repeated() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![Instruction::ProceduresCall {
                    mutation: json!({"proccode": LOG, "argumentids": "[\"extra\",\"arg0\"]"}),
                    arguments: HashMap::from([
                        ("extra".to_string(), value("not this")),
                        ("arg0".to_string(), Instruction::MouseX),
                    ]),
                }],
            )
            .unwrap();
        tick(&mut runtime);
        let id = runtime.pending_requests().next().unwrap().0;
        runtime.drain_effects();
        tick(&mut runtime);
        assert!(runtime.drain_effects().is_empty());
        runtime
            .respond(id, Response::Value(Value::Number(42.0)))
            .unwrap();
        tick(&mut runtime);
        assert!(
            matches!(runtime.drain_effects().as_slice(), [Effect::Debugger { event: DebuggerEvent::Log { message, .. }, .. }] if message == "42")
        );
        assert!(runtime.is_idle());
    }

    #[test]
    fn ordinary_log_procedures_are_not_intercepted() {
        let project = Project::load_json(&json!({"targets": [{"name": "Sprite", "blocks": {
            "definition": {"opcode": "procedures_definition", "inputs": {"custom_block": [1, "prototype"]}, "next": "increment"},
            "prototype": {"opcode": "procedures_prototype", "mutation": {"proccode": "log %s", "argumentids": "[\"arg0\"]", "argumentnames": "[\"text\"]", "argumentdefaults": "[\"\"]"}},
            "increment": {"opcode": "control_incr_counter"}
        }}]})).unwrap();
        let target = *project.targets.keys().next().unwrap();
        let mut runtime = Runtime::new(project);
        runtime
            .run_instructions(&target, vec![call("log %s", Some(value("ordinary")))])
            .unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.scratch.counter, 1.0);
        assert!(runtime.drain_effects().is_empty());
    }

    #[test]
    fn unknown_decorated_procedures_still_follow_the_missing_call_policy() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![call(
                    "\u{200b}log\u{200b} %s",
                    Some(value("not an addon signature")),
                )],
            )
            .unwrap();
        tick(&mut runtime);
        assert!(matches!(
            runtime.drain_effects().as_slice(),
            [Effect::Warning { .. }]
        ));
    }

    #[test]
    fn timers_use_runtime_time_and_restarting_a_label_replaces_its_start() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(&target, vec![call(START, Some(value("work")))])
            .unwrap();
        tick(&mut runtime);
        ready(runtime.tick_with_delta(Duration::from_millis(300))).unwrap();
        runtime
            .run_instructions(&target, vec![call(START, Some(value("work")))])
            .unwrap();
        tick(&mut runtime);
        runtime.drain_effects();
        runtime
            .run_instructions(
                &target,
                vec![
                    call(STOP, Some(value("work"))),
                    call(STOP, Some(value("work"))),
                ],
            )
            .unwrap();
        ready(runtime.tick_with_delta(Duration::from_millis(200))).unwrap();
        assert!(
            matches!(runtime.drain_effects().as_slice(), [Effect::Debugger { event: DebuggerEvent::TimerStopped { label, elapsed }, .. }]
            if label == "work" && *elapsed == Duration::from_millis(200))
        );
    }

    #[test]
    fn breakpoints_and_legacy_pause_stop_all_threads_and_freeze_time() {
        for code in [BREAKPOINT, "sa-pause"] {
            let (mut runtime, target) = runtime();
            runtime
                .run_instructions(&target, vec![call(code, None), Instruction::IncrCounter])
                .unwrap();
            runtime
                .run_instructions(&target, vec![Instruction::IncrCounter])
                .unwrap();
            tick(&mut runtime);
            assert!(runtime.is_debugger_paused());
            assert_eq!(runtime.scratch.counter, 0.0);
            let id = runtime.pending_requests().next().unwrap().0;
            assert_eq!(
                runtime.pending_request(id).unwrap().request,
                Request::DebuggerPause
            );
            assert!(matches!(
                runtime.respond(id, Response::Answer("wrong type".to_string())),
                Err(RuntimeError::InvalidResponse(_))
            ));
            ready(runtime.tick_with_delta(Duration::from_secs(10))).unwrap();
            assert_eq!(runtime.time(), Duration::ZERO);
            assert_eq!(runtime.scratch.counter, 0.0);
            runtime.resume_debugger().unwrap();
            assert!(!runtime.is_debugger_paused());
            tick(&mut runtime);
            assert_eq!(runtime.scratch.counter, 2.0);
            assert!(runtime.is_idle());
        }
    }

    #[test]
    fn cancelling_a_breakpoint_unpauses_the_other_threads() {
        let (mut runtime, target) = runtime();
        let paused = runtime
            .run_instructions(&target, vec![call(BREAKPOINT, None)])
            .unwrap();
        runtime
            .run_instructions(&target, vec![Instruction::IncrCounter])
            .unwrap();
        tick(&mut runtime);
        runtime.stop_thread(paused);
        assert!(!runtime.is_debugger_paused());
        tick(&mut runtime);
        assert_eq!(runtime.scratch.counter, 1.0);
        assert!(runtime.is_idle());
    }

    #[test]
    fn paused_ticks_do_not_expire_waiting_scripts() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::Wait {
                        duration: Box::new(Instruction::Literal(Value::Number(1.0))),
                    },
                    Instruction::IncrCounter,
                ],
            )
            .unwrap();
        runtime
            .run_instructions(&target, vec![call(BREAKPOINT, None)])
            .unwrap();
        tick(&mut runtime);
        ready(runtime.tick_with_delta(Duration::from_secs(20))).unwrap();
        runtime.resume_debugger().unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.scratch.counter, 0.0);
        ready(runtime.tick_with_delta(Duration::from_secs(1))).unwrap();
        assert_eq!(runtime.scratch.counter, 1.0);
    }

    #[test]
    fn green_flag_clears_debugger_timers_and_breakpoints() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![call(START, Some(value("work"))), call(BREAKPOINT, None)],
            )
            .unwrap();
        tick(&mut runtime);
        ready(runtime.green_flag()).unwrap();
        assert!(!runtime.is_debugger_paused());
        runtime.drain_effects();
        runtime
            .run_instructions(&target, vec![call(STOP, Some(value("work")))])
            .unwrap();
        tick(&mut runtime);
        assert!(runtime.drain_effects().is_empty());
    }
}
