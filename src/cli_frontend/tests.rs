use super::*;
use crate::{Instruction, Project, Request, TargetId};
use serde_json::json;
use std::{
    future::Future,
    io::Cursor,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Buffer {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future)
}

fn runtime() -> (Runtime, TargetId) {
    let project = Project::load_json(
        &json!({"targets": [{"name": "Sprite", "blocks": {}, "sounds": [{"name": "meow"}]}]}),
    )
    .unwrap();
    let id = *project.targets.keys().next().unwrap();
    (Runtime::new(project), id)
}

#[test]
fn unsupported_requests_are_logged_and_fail_instead_of_hanging() {
    let (mut runtime, target) = runtime();
    let log = Buffer::default();
    let mut frontend =
        CliFrontend::with_io(Cursor::new(Vec::new()), Buffer::default(), log.clone());
    frontend.frame_interval = Duration::ZERO;
    runtime
        .run_instructions(
            &target,
            vec![Instruction::Request {
                request: Request::External {
                    name: "unsupported-operation".to_string(),
                    arguments: vec![],
                },
            }],
        )
        .unwrap();
    assert!(matches!(
        block_on(frontend.run(&mut runtime)),
        Err(CliFrontendError::Runtime(
            RuntimeError::RequestFailed { .. }
        ))
    ));
    assert!(runtime.is_idle());
    let log = log.text();
    assert!(log.contains("[effect: unsupported] Request"));
    assert!(log.contains("unsupported-operation"));
    assert!(log.contains("thread_id:"));
}

#[test]
fn io_errors_are_reported_instead_of_panicking() {
    struct BrokenWriter;
    impl Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let (mut runtime, target) = runtime();
    let mut frontend =
        CliFrontend::with_io(Cursor::new(Vec::new()), Buffer::default(), BrokenWriter);
    runtime
        .run_instructions(
            &target,
            vec![Instruction::Request {
                request: Request::External {
                    name: "test".to_string(),
                    arguments: vec![],
                },
            }],
        )
        .unwrap();
    assert!(matches!(
        block_on(frontend.tick(&mut runtime)),
        Err(CliFrontendError::Io(_))
    ));
}

#[test]
fn quiet_logs_do_not_hide_returned_errors_or_drop_request_effects() {
    let (mut runtime, target) = runtime();
    assert!(!runtime.config.quiet_logs);
    runtime.config.quiet_logs = true;
    runtime
        .run_instructions(
            &target,
            vec![Instruction::Request {
                request: Request::External {
                    name: "unsupported".to_string(),
                    arguments: vec![],
                },
            }],
        )
        .unwrap();
    block_on(runtime.tick()).unwrap();
    // The runtime still emits its request: only frontend presentation is quiet.
    assert_eq!(runtime.pending_requests().count(), 1);
    let log = Buffer::default();
    let mut frontend =
        CliFrontend::with_io(Cursor::new(Vec::new()), Buffer::default(), log.clone());
    assert!(matches!(
        block_on(frontend.run(&mut runtime)),
        Err(CliFrontendError::Runtime(
            RuntimeError::RequestFailed { .. }
        ))
    ));
    assert!(runtime.is_idle());
    assert_eq!(runtime.pending_requests().count(), 0);
    assert!(log.text().is_empty());
}

#[test]
fn quiet_logs_can_be_set_at_construction_and_toggled_between_ticks() {
    let project =
        Project::load_json(&json!({"targets": [{"name": "Sprite", "blocks": {}}]})).unwrap();
    let target = *project.targets.keys().next().unwrap();
    let mut runtime = Runtime::with_config(
        project,
        crate::RuntimeConfig {
            quiet_logs: true,
            ..crate::RuntimeConfig::default()
        },
    );
    let log = Buffer::default();
    let mut frontend =
        CliFrontend::with_io(Cursor::new(Vec::new()), Buffer::default(), log.clone());
    for quiet in [true, false] {
        runtime.config.quiet_logs = quiet;
        let thread = runtime
            .run_instructions(
                &target,
                vec![Instruction::Request {
                    request: Request::External {
                        name: "cancelled".to_string(),
                        arguments: vec![],
                    },
                }],
            )
            .unwrap();
        block_on(runtime.tick()).unwrap();
        runtime.stop_thread(thread);
        frontend.process_effects(&mut runtime).unwrap();
        assert_eq!(log.text().is_empty(), quiet);
        assert_eq!(runtime.pending_requests().count(), 0);
    }
    assert!(log.text().contains("[effect] CancelRequest"));
}

#[test]
fn cancellation_effects_are_logged_without_scratch_support_too() {
    let (mut runtime, target) = runtime();
    let thread = runtime
        .run_instructions(
            &target,
            vec![Instruction::Request {
                request: Request::External {
                    name: "test".to_string(),
                    arguments: vec![],
                },
            }],
        )
        .unwrap();
    block_on(runtime.tick()).unwrap();
    runtime.stop_thread(thread);
    let log = Buffer::default();
    let mut frontend =
        CliFrontend::with_io(Cursor::new(Vec::new()), Buffer::default(), log.clone());
    frontend.process_effects(&mut runtime).unwrap();
    assert!(log.text().contains("[effect] CancelRequest"));
}

#[cfg(feature = "scratch")]
mod scratch {
    use super::*;
    use crate::{Field, Value};

    fn text(text: &str) -> Box<Instruction> {
        Box::new(Instruction::Literal(Value::String(text.to_string())))
    }

    #[test]
    fn quiet_logs_keep_speech_and_prompts_while_handling_fallbacks() {
        let (mut runtime, target) = runtime();
        runtime.config.quiet_logs = true;
        let output = Buffer::default();
        let log = Buffer::default();
        let mut frontend =
            CliFrontend::with_io(Cursor::new(b"Ada\n".to_vec()), output.clone(), log.clone());
        frontend.headless = true;
        frontend.frame_interval = Duration::ZERO;
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::ProceduresCall {
                        mutation: json!({"proccode": "missing"}),
                        arguments: std::collections::HashMap::new(),
                    },
                    Instruction::PenDown,
                    Instruction::SetX {
                        x: Box::new(Instruction::Literal(Value::Number(240.0))),
                    },
                    Instruction::IfOnEdgeBounce,
                    Instruction::TouchingColor {
                        color: text("#ff0000"),
                    },
                    Instruction::Goto {
                        to: text("_mouse_"),
                    },
                    Instruction::DaysSince2000,
                    Instruction::Say {
                        message: text("hello"),
                    },
                    Instruction::Think {
                        message: text("thinking"),
                    },
                    Instruction::AskAndWait {
                        question: text("Name?"),
                    },
                    Instruction::Say {
                        message: Box::new(Instruction::Answer),
                    },
                    Instruction::IncrCounter,
                ],
            )
            .unwrap();
        block_on(frontend.run(&mut runtime)).unwrap();
        assert_eq!(output.text(), "hello\nthinking\nName? Ada\n");
        assert!(log.text().is_empty(), "{}", log.text());
        assert_eq!(frontend.headless_fallbacks, 2);
        assert_eq!(runtime.scratch.counter, 1.0);
        assert!(runtime.is_idle());
        assert_eq!(runtime.pending_requests().count(), 0);
    }

    #[test]
    fn fixed_step_clock_advances_waits_without_wall_clock_sleeps() {
        let (mut runtime, target) = runtime();
        let mut frontend = CliFrontend::with_io(
            Cursor::new(Vec::new()),
            Buffer::default(),
            Buffer::default(),
        );
        frontend.frame_interval = Duration::ZERO;
        frontend.frame_delta = Some(Duration::from_millis(250));
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
        block_on(frontend.tick(&mut runtime)).unwrap();
        assert_eq!(runtime.time(), Duration::from_millis(250));
        assert_eq!(runtime.scratch.counter, 0.0);
        for _ in 0..4 {
            block_on(frontend.tick(&mut runtime)).unwrap();
        }
        assert_eq!(runtime.time(), Duration::from_millis(1250));
        assert_eq!(runtime.scratch.counter, 1.0);
        assert!(runtime.is_idle());
    }

    #[test]
    fn headless_sensing_defaults_are_opt_in_and_explicitly_logged() {
        for headless in [false, true] {
            let (mut runtime, target) = runtime();
            let log = Buffer::default();
            let mut frontend =
                CliFrontend::with_io(Cursor::new(Vec::new()), Buffer::default(), log.clone());
            frontend.headless = headless;
            frontend.frame_interval = Duration::ZERO;
            runtime
                .run_instructions(
                    &target,
                    vec![
                        Instruction::TouchingColor {
                            color: text("#ff0000"),
                        },
                        Instruction::IncrCounter,
                    ],
                )
                .unwrap();
            let result = block_on(frontend.run(&mut runtime));
            if headless {
                result.unwrap();
                assert_eq!(runtime.scratch.counter, 1.0);
                assert_eq!(frontend.headless_fallbacks, 1);
                assert!(log.text().contains("[effect: headless fallback] Request"));
            } else {
                assert!(matches!(
                    result,
                    Err(CliFrontendError::Runtime(
                        RuntimeError::RequestFailed { .. }
                    ))
                ));
                assert_eq!(runtime.scratch.counter, 0.0);
                assert_eq!(frontend.headless_fallbacks, 0);
            }
        }
    }

    #[test]
    fn calendar_sensors_use_the_local_system_clock() {
        for (component, min, max) in [
            ("year", 2000.0, 9999.0),
            ("month", 1.0, 12.0),
            ("date", 1.0, 31.0),
            ("dayofweek", 1.0, 7.0),
            ("hour", 0.0, 23.0),
            ("minute", 0.0, 59.0),
            ("second", 0.0, 60.0),
        ] {
            let response =
                CliFrontend::clock_sensor(&crate::Sensor::Current(component.to_string())).unwrap();
            assert!(
                matches!(response, Response::Value(Value::Number(value)) if (min..=max).contains(&value))
            );
        }
        assert!(
            matches!(CliFrontend::clock_sensor(&crate::Sensor::DaysSince2000), Some(Response::Value(Value::Number(days))) if days > 0.0)
        );
    }

    #[test]
    fn missing_procedure_warnings_are_printed_and_the_script_continues() {
        let (mut runtime, target) = runtime();
        let log = Buffer::default();
        let output = Buffer::default();
        let mut frontend =
            CliFrontend::with_io(Cursor::new(Vec::new()), output.clone(), log.clone());
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::ProceduresCall {
                        mutation: json!({"proccode": "missing"}),
                        arguments: std::collections::HashMap::new(),
                    },
                    Instruction::Say {
                        message: text("still running"),
                    },
                ],
            )
            .unwrap();
        block_on(frontend.run(&mut runtime)).unwrap();
        assert_eq!(output.text(), "still running\n");
        assert!(
            log.text()
                .contains("[warning] procedure \"missing\" was not found; skipping call")
        );
    }

    #[test]
    fn strict_missing_procedure_errors_still_log_the_warning() {
        let (mut runtime, target) = runtime();
        runtime.config.error_on_missing_procedures = true;
        let log = Buffer::default();
        let mut frontend =
            CliFrontend::with_io(Cursor::new(Vec::new()), Buffer::default(), log.clone());
        runtime
            .run_instructions(
                &target,
                vec![Instruction::ProceduresCall {
                    mutation: json!({"proccode": "missing"}),
                    arguments: std::collections::HashMap::new(),
                }],
            )
            .unwrap();
        assert!(matches!(
            block_on(frontend.run(&mut runtime)),
            Err(CliFrontendError::Runtime(RuntimeError::ProcedureNotFound(
                _
            )))
        ));
        assert!(log.text().contains("[warning]"));
        assert!(log.text().contains("strict mode"));
    }

    #[cfg(feature = "scratch-addons-debugger")]
    #[test]
    fn addon_debugger_logs_and_breakpoints_are_handled_by_the_cli() {
        for quiet_logs in [false, true] {
            let (mut runtime, target) = runtime();
            runtime.config.quiet_logs = quiet_logs;
            let log = Buffer::default();
            let output = Buffer::default();
            let mut frontend =
                CliFrontend::with_io(Cursor::new(b"\n".to_vec()), output.clone(), log.clone());
            frontend.frame_interval = Duration::from_millis(1);
            let mut code = Vec::new();
            for level in ["log", "warn", "error"] {
                code.push(Instruction::ProceduresCall {
                mutation: json!({"proccode": format!("\u{200b}\u{200b}{level}\u{200b}\u{200b} %s"), "argumentids": "[\"arg0\"]"}),
                arguments: std::collections::HashMap::from([("arg0".to_string(), Instruction::Literal(Value::String(format!("a {level} message"))))]),
            });
            }
            code.push(Instruction::ProceduresCall {
                mutation: json!({"proccode": "\u{200b}\u{200b}breakpoint\u{200b}\u{200b}"}),
                arguments: std::collections::HashMap::new(),
            });
            code.push(Instruction::IncrCounter);
            runtime.run_instructions(&target, code).unwrap();
            block_on(frontend.run(&mut runtime)).unwrap();
            assert!(runtime.is_idle());
            assert!(!runtime.is_debugger_paused());
            assert_eq!(runtime.scratch.counter, 1.0);
            for level in ["log", "warn", "error"] {
                assert!(
                    log.text()
                        .contains(&format!("[debugger:{level}] a {level} message"))
                );
            }
            assert!(!log.text().contains("unsupported"));
            assert!(output.text().contains("press Enter to continue"));
        }
    }

    #[test]
    fn bounce_fallback_handles_edges_corners_overshoot_and_inward_motion() {
        for (x, y, direction, expected_x, expected_y, expected_direction) in [
            (240.0, 0.0, 90.0, 240.0, 0.0, -90.0),
            (-240.0, 0.0, -90.0, -240.0, 0.0, 90.0),
            (0.0, 180.0, 0.0, 0.0, 180.0, 180.0),
            (0.0, -180.0, 180.0, 0.0, -180.0, 0.0),
            (240.0, 180.0, 45.0, 240.0, 180.0, -135.0),
            (250.0, 190.0, 45.0, 240.0, 180.0, -135.0),
            (240.0, 0.0, -90.0, 240.0, 0.0, -90.0),
            (0.0, 180.0, 180.0, 0.0, 180.0, 180.0),
            (0.0, 0.0, 45.0, 0.0, 0.0, 45.0),
        ] {
            assert_eq!(
                CliFrontend::bounce_response(x, y, direction),
                Response::Motion {
                    x: expected_x,
                    y: expected_y,
                    direction: expected_direction,
                }
            );
        }
    }

    #[test]
    fn edge_bounce_instructions_continue_instead_of_failing_the_cli() {
        let (mut runtime, target) = runtime();
        let log = Buffer::default();
        let mut frontend =
            CliFrontend::with_io(Cursor::new(Vec::new()), Buffer::default(), log.clone());
        frontend.frame_interval = Duration::ZERO;
        let number = |n| Box::new(Instruction::Literal(Value::Number(n)));
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::GotoXY {
                        x: number(240.0),
                        y: number(0.0),
                    },
                    Instruction::PointInDirection {
                        direction: number(90.0),
                    },
                    Instruction::IfOnEdgeBounce,
                    Instruction::MoveSteps {
                        steps: number(10.0),
                    },
                ],
            )
            .unwrap();
        block_on(frontend.run(&mut runtime)).unwrap();
        let crate::Target::Normal { state, .. } = &runtime.project.targets[&target] else {
            panic!()
        };
        assert!((state.x - 230.0).abs() < 1e-10);
        assert!(state.y.abs() < 1e-10);
        assert_eq!(state.direction, -90.0);
        assert!(runtime.is_idle());
        assert_eq!(runtime.pending_requests().count(), 0);
        assert!(
            log.text()
                .contains("[effect: approximate stage bounds] Request")
        );
        assert_eq!(log.text().matches("Bounce {").count(), 1);
    }

    #[test]
    fn asks_are_answered_by_the_shared_frontend_and_bubbles_are_printed() {
        let (mut runtime, target) = runtime();
        let output = Buffer::default();
        let log = Buffer::default();
        let mut frontend = CliFrontend::with_io(
            Cursor::new(b"Ada\r\n".to_vec()),
            output.clone(),
            log.clone(),
        );
        frontend.frame_interval = Duration::from_millis(1);
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::AskAndWait {
                        question: text("Name?"),
                    },
                    Instruction::Say {
                        message: Box::new(Instruction::Answer),
                    },
                ],
            )
            .unwrap();
        block_on(frontend.run(&mut runtime)).unwrap();
        assert!(runtime.is_idle());
        assert_eq!(runtime.scratch.answer, "Ada");
        assert_eq!(output.text(), "Name? Ada\n");
        let log = log.text();
        assert!(log.contains("[effect] Request"));
        assert!(log.contains("Ask { question:"));
        assert!(log.contains("[effect] Bubble"));
        assert_eq!(log.matches("Ask { question:").count(), 1);
    }

    #[test]
    fn unavailable_input_fails_the_request_without_hanging() {
        let (mut runtime, target) = runtime();
        let mut frontend = CliFrontend::with_io(
            Cursor::new(Vec::new()),
            Buffer::default(),
            Buffer::default(),
        );
        frontend.frame_interval = Duration::from_millis(1);
        runtime
            .run_instructions(
                &target,
                vec![Instruction::AskAndWait {
                    question: text("Input?"),
                }],
            )
            .unwrap();
        let error = block_on(frontend.run(&mut runtime)).unwrap_err();
        assert!(
            matches!(error, CliFrontendError::Runtime(RuntimeError::RequestFailed { message, .. }) if message.contains("stdin closed"))
        );
    }

    #[test]
    fn effects_the_console_cannot_render_are_not_discarded_silently() {
        let (mut runtime, target) = runtime();
        let output = Buffer::default();
        let log = Buffer::default();
        let mut frontend =
            CliFrontend::with_io(Cursor::new(Vec::new()), output.clone(), log.clone());
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::PenDown,
                    Instruction::PlaySound {
                        sound: text("meow"),
                    },
                    Instruction::SetX {
                        x: Box::new(Instruction::Literal(Value::Number(42.0))),
                    },
                    Instruction::ShowVariable {
                        variable: Field {
                            value: Value::String("score".to_string()),
                            id: Some("score".to_string()),
                        },
                    },
                    Instruction::Say {
                        message: text("hello"),
                    },
                ],
            )
            .unwrap();
        block_on(frontend.run(&mut runtime)).unwrap();
        assert_eq!(output.text(), "hello\n");
        let log = log.text();
        for effect in ["Pen", "PlaySound", "TargetChanged", "Monitor"] {
            assert!(
                log.contains(&format!("[effect: unsupported] {effect}")),
                "{log}"
            );
        }
        assert!(log.contains("[effect] Bubble"));
    }

    #[test]
    fn an_ask_cancelled_in_the_same_tick_does_not_prompt() {
        let (mut runtime, target) = runtime();
        let output = Buffer::default();
        let log = Buffer::default();
        let mut frontend =
            CliFrontend::with_io(Cursor::new(Vec::new()), output.clone(), log.clone());
        runtime
            .run_instructions(
                &target,
                vec![Instruction::AskAndWait {
                    question: text("cancelled"),
                }],
            )
            .unwrap();
        runtime
            .run_instructions(
                &target,
                vec![Instruction::Stop {
                    option: Field {
                        value: Value::String("all".to_string()),
                        id: None,
                    },
                }],
            )
            .unwrap();
        block_on(frontend.run(&mut runtime)).unwrap();
        assert!(frontend.worker.is_none());
        assert!(output.text().is_empty());
        assert!(log.text().contains("CancelRequest"));
        assert!(log.text().contains("[effect: unsupported] StopAllSounds"));
    }

    #[test]
    fn late_answers_to_cancelled_questions_are_ignored() {
        let (mut runtime, target) = runtime();
        let thread = runtime
            .run_instructions(
                &target,
                vec![Instruction::AskAndWait {
                    question: text("cancelled"),
                }],
            )
            .unwrap();
        block_on(runtime.tick()).unwrap();
        let id = runtime.pending_requests().next().unwrap().0;
        runtime.drain_effects();
        let mut frontend = CliFrontend::with_io(
            Cursor::new(Vec::new()),
            Buffer::default(),
            Buffer::default(),
        );
        let (send, _) = mpsc::channel();
        let (answers, receive) = mpsc::channel();
        frontend.worker = Some(InputWorker { send, receive });
        frontend.questions.insert(
            id,
            PendingInput {
                active: Arc::new(AtomicBool::new(true)),
                kind: InputKind::Answer,
            },
        );
        runtime.stop_thread(thread);
        frontend.process_effects(&mut runtime).unwrap();
        answers.send((id, Ok("too late".to_string()))).unwrap();
        frontend.process_effects(&mut runtime).unwrap();
        assert!(runtime.scratch.answer.is_empty());
        assert!(frontend.questions.is_empty());
    }

    #[test]
    fn waiting_for_console_input_does_not_stop_other_scripts() {
        struct BlockingInput {
            started: mpsc::Sender<()>,
            release: mpsc::Receiver<String>,
        }
        impl io::Read for BlockingInput {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("use read_line"))
            }
        }
        impl BufRead for BlockingInput {
            fn fill_buf(&mut self) -> io::Result<&[u8]> {
                Err(io::Error::other("use read_line"))
            }
            fn consume(&mut self, _: usize) {}
            fn read_line(&mut self, line: &mut String) -> io::Result<usize> {
                let _ = self.started.send(());
                let answer = self
                    .release
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(io::Error::other)?;
                line.push_str(&answer);
                Ok(answer.len())
            }
        }
        let (started, reading) = mpsc::channel();
        let (release, input) = mpsc::channel();
        let (mut runtime, target) = runtime();
        let mut frontend = CliFrontend::with_io(
            BlockingInput {
                started,
                release: input,
            },
            Buffer::default(),
            Buffer::default(),
        );
        runtime
            .run_instructions(
                &target,
                vec![Instruction::AskAndWait {
                    question: text("waiting"),
                }],
            )
            .unwrap();
        block_on(frontend.tick(&mut runtime)).unwrap();
        reading.recv_timeout(Duration::from_secs(1)).unwrap();
        runtime
            .run_instructions(&target, vec![Instruction::IncrCounter])
            .unwrap();
        block_on(frontend.tick(&mut runtime)).unwrap();
        assert_eq!(runtime.scratch.counter, 1.0);
        release.send("done\n".to_string()).unwrap();
        block_on(frontend.run(&mut runtime)).unwrap();
        assert_eq!(runtime.scratch.answer, "done");
    }
}
