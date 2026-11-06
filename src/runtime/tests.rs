use super::*;
use crate::{Instruction, Project, TargetId, Value};
use serde_json::json;
use std::{
    future::Future,
    task::{Context, Poll, Waker},
    time::Duration,
};

#[cfg(feature = "scratch")]
mod hidden;
#[cfg(all(
    feature = "scratch",
    feature = "cli-frontend",
    feature = "scratch-addons-debugger"
))]
mod parity;
#[cfg(feature = "scratch")]
mod procedures;

fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("runtime operations must not wait on the frontend future"),
    }
}

fn runtime() -> (Runtime, TargetId) {
    let project = Project::load_json(
        &json!({"targets": [{"isStage": false, "name": "Sprite", "blocks": {}, "sounds": [{"name": "meow"}]}]}),
    )
    .unwrap();
    let id = *project.targets.keys().next().unwrap();
    (Runtime::new(project), id)
}

fn tick(runtime: &mut Runtime) {
    ready(runtime.tick_with_delta(Duration::ZERO)).unwrap();
}

#[test]
fn base_requests_emit_once_resume_and_are_consumed() {
    let (mut runtime, target) = runtime();
    let thread = runtime
        .run_instructions(
            &target,
            vec![Instruction::Request {
                request: Request::External {
                    name: "test".to_string(),
                    arguments: vec![Value::Number(2.0)],
                },
            }],
        )
        .unwrap();
    tick(&mut runtime);
    let effects = runtime.drain_effects();
    let Effect::Request {
        id,
        thread_id,
        request,
        ..
    } = &effects[0]
    else {
        panic!()
    };
    assert_eq!(*thread_id, thread);
    assert!(matches!(request, Request::External { .. }));
    let id = *id;
    for _ in 0..5 {
        tick(&mut runtime);
    }
    assert!(runtime.drain_effects().is_empty());
    assert_eq!(runtime.pending_requests().count(), 1);
    runtime.respond(id, Response::Completed).unwrap();
    assert!(matches!(
        runtime.respond(id, Response::Completed),
        Err(RuntimeError::AlreadyResponded(_))
    ));
    tick(&mut runtime);
    assert!(runtime.is_idle());
    assert!(runtime.pending_request(id).is_none());
    assert!(matches!(
        runtime.respond(id, Response::Completed),
        Err(RuntimeError::UnknownRequest(_))
    ));
}

#[test]
fn stopping_threads_cancels_frontend_requests() {
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
    tick(&mut runtime);
    let id = runtime.pending_requests().next().unwrap().0;
    runtime.drain_effects();
    runtime.stop_thread(thread);
    assert_eq!(runtime.drain_effects(), vec![Effect::CancelRequest { id }]);
    assert!(runtime.is_idle());
    assert!(matches!(
        runtime.respond(id, Response::Completed),
        Err(RuntimeError::UnknownRequest(_))
    ));
}

#[test]
fn failed_requests_finish_the_thread_and_return_an_error() {
    let (mut runtime, target) = runtime();
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
    tick(&mut runtime);
    let id = runtime.pending_requests().next().unwrap().0;
    runtime
        .respond(id, Response::Failed("unavailable".to_string()))
        .unwrap();
    assert!(matches!(
        ready(runtime.tick()),
        Err(RuntimeError::RequestFailed { .. })
    ));
    assert!(runtime.is_idle());
}

#[test]
fn empty_forever_loops_cooperate_and_do_not_grow_the_stack() {
    let (mut runtime, target) = runtime();
    runtime.tasks_per_tick = 10;
    let id = runtime
        .run_instructions(&target, vec![Instruction::Forever { body: vec![] }])
        .unwrap();
    for _ in 0..100 {
        tick(&mut runtime);
    }
    let thread = runtime.scheduler.thread(id).unwrap();
    assert!(thread.tasks.len() <= 2);
    assert!(thread.values.is_empty());
    runtime.stop_all();
    assert!(runtime.is_idle());
}

#[cfg(feature = "scratch")]
mod scratch {
    use super::*;
    use crate::Field;

    fn number(n: f64) -> Box<Instruction> {
        Box::new(Instruction::Literal(Value::Number(n)))
    }
    fn text(s: &str) -> Box<Instruction> {
        Box::new(Instruction::Literal(Value::String(s.to_string())))
    }
    fn field(name: &str) -> Field {
        Field {
            value: Value::String(name.to_string()),
            id: Some(name.to_string()),
        }
    }

    #[test]
    fn ask_waits_without_blocking_other_threads_then_answer_is_available() {
        let (mut runtime, target) = runtime();
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
        runtime
            .run_instructions(&target, vec![Instruction::SetX { x: number(42.0) }])
            .unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.normal(target).unwrap().x, 42.0);
        let effects = runtime.drain_effects();
        let request_id = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Request {
                    id,
                    request: Request::Ask { question },
                    ..
                } => {
                    assert_eq!(question, "Name?");
                    Some(*id)
                }
                _ => None,
            })
            .unwrap();
        assert!(matches!(
            runtime.respond(request_id, Response::Completed),
            Err(RuntimeError::InvalidResponse(_))
        ));
        for _ in 0..10 {
            tick(&mut runtime);
        }
        assert!(runtime.drain_effects().is_empty());
        runtime
            .respond(request_id, Response::Answer("Ada".to_string()))
            .unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.scratch.answer, "Ada");
        assert_eq!(
            runtime.drain_effects(),
            vec![Effect::Bubble {
                target_id: target,
                kind: BubbleKind::Say,
                text: Some("Ada".to_string())
            }]
        );
        assert!(runtime.is_idle());
    }

    #[test]
    fn nested_asynchronous_reporters_resume_without_reissuing_earlier_inputs() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![Instruction::SetX {
                    x: Box::new(Instruction::Add {
                        num1: Box::new(Instruction::MouseX),
                        num2: Box::new(Instruction::MouseY),
                    }),
                }],
            )
            .unwrap();
        tick(&mut runtime);
        let first = runtime.pending_requests().next().unwrap().0;
        runtime
            .respond(first, Response::Value(Value::Number(10.0)))
            .unwrap();
        tick(&mut runtime);
        let second = runtime.pending_requests().next().unwrap().0;
        assert_ne!(first, second);
        assert!(matches!(
            runtime.pending_request(second).unwrap().request,
            Request::Sense(Sensor::MouseY)
        ));
        runtime
            .respond(second, Response::Value(Value::Number(20.0)))
            .unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.normal(target).unwrap().x, 30.0);
        assert!(runtime.is_idle());
    }

    #[test]
    fn waits_and_timed_bubbles_use_runtime_time() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::SayForSecs {
                        message: text("hello"),
                        secs: number(2.0),
                    },
                    Instruction::SetX { x: number(99.0) },
                ],
            )
            .unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.drain_effects().len(), 1);
        ready(runtime.tick_with_delta(Duration::from_secs(1))).unwrap();
        assert_eq!(runtime.normal(target).unwrap().x, 0.0);
        assert!(runtime.drain_effects().is_empty());
        ready(runtime.tick_with_delta(Duration::from_secs(1))).unwrap();
        assert_eq!(runtime.normal(target).unwrap().x, 99.0);
        assert!(runtime.drain_effects().contains(&Effect::Bubble {
            target_id: target,
            kind: BubbleKind::Say,
            text: None
        }));
        assert!(runtime.is_idle());
    }

    #[test]
    fn repeats_and_repeat_until_do_not_accumulate_operand_values() {
        let (mut runtime, target) = runtime();
        let variable = field("count");
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::SetVariableTo {
                        variable: variable.clone(),
                        value: number(0.0),
                    },
                    Instruction::RepeatUntil {
                        condition: Box::new(Instruction::GreaterThan {
                            operand1: Box::new(Instruction::Variable {
                                variable: variable.clone(),
                            }),
                            operand2: number(99.0),
                        }),
                        body: vec![Instruction::ChangeVariableBy {
                            variable: variable.clone(),
                            value: number(1.0),
                        }],
                    },
                    Instruction::Repeat {
                        times: number(3.0),
                        body: vec![Instruction::ChangeVariableBy {
                            variable: variable.clone(),
                            value: number(1.0),
                        }],
                    },
                ],
            )
            .unwrap();
        for _ in 0..110 {
            tick(&mut runtime);
        }
        assert_eq!(
            runtime.scratch.targets[&target].variables["count"].value,
            Value::Number(103.0)
        );
        assert!(runtime.is_idle());
    }

    #[test]
    fn glides_interpolate_across_ticks() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![Instruction::GlideSecsToXY {
                    secs: number(2.0),
                    x: number(100.0),
                    y: number(40.0),
                }],
            )
            .unwrap();
        tick(&mut runtime);
        ready(runtime.tick_with_delta(Duration::from_secs(1))).unwrap();
        assert_eq!(runtime.position(target), (50.0, 20.0));
        ready(runtime.tick_with_delta(Duration::from_secs(1))).unwrap();
        assert_eq!(runtime.position(target), (100.0, 40.0));
        assert!(runtime.is_idle());
    }

    #[test]
    fn list_operations_work_and_respect_scratch_indices() {
        let (mut runtime, target) = runtime();
        let list = field("items");
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::AddToList {
                        list: list.clone(),
                        item: text("a"),
                    },
                    Instruction::AddToList {
                        list: list.clone(),
                        item: text("c"),
                    },
                    Instruction::InsertAtList {
                        list: list.clone(),
                        index: number(2.0),
                        item: text("b"),
                    },
                    Instruction::ReplaceItemOfList {
                        list: list.clone(),
                        index: text("last"),
                        item: text("d"),
                    },
                    Instruction::DeleteOfList {
                        list: list.clone(),
                        index: number(0.0),
                    },
                    Instruction::Say {
                        message: Box::new(Instruction::ListContents { list: list.clone() }),
                    },
                ],
            )
            .unwrap();
        tick(&mut runtime);
        assert_eq!(
            runtime.scratch.targets[&target].lists["items"].items.len(),
            3
        );
        assert!(runtime.drain_effects().contains(&Effect::Bubble {
            target_id: target,
            kind: BubbleKind::Say,
            text: Some("abd".to_string())
        }));
    }

    #[test]
    fn broadcast_and_wait_joins_only_matching_receivers_including_pending_requests() {
        let project = Project::load_json(&json!({"targets": [{"name": "Sprite", "blocks": {
            "receive": {"opcode": "event_whenbroadcastreceived", "fields": {"BROADCAST_OPTION": ["hello", "message"]}, "topLevel": true, "next": "ask"},
            "ask": {"opcode": "sensing_askandwait", "inputs": {"QUESTION": [1, [10, "continue?"]]}},
            "other": {"opcode": "event_whenbroadcastreceived", "fields": {"BROADCAST_OPTION": ["other", "other-id"]}, "topLevel": true, "next": "bad"},
            "bad": {"opcode": "motion_setx", "inputs": {"X": [1, [4, -1]]}}
        }}]})).unwrap();
        let target = *project.targets.keys().next().unwrap();
        let mut runtime = Runtime::new(project);
        let parent = runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::BroadcastAndWait {
                        broadcast: text("hello"),
                    },
                    Instruction::SetX { x: number(100.0) },
                ],
            )
            .unwrap();
        tick(&mut runtime);
        assert!(matches!(
            runtime.scheduler.thread(parent).unwrap().status,
            ThreadStatus::Joining { .. }
        ));
        tick(&mut runtime);
        assert_eq!(runtime.position(target).0, 0.0);
        let request = runtime.pending_requests().next().unwrap().0;
        runtime
            .respond(request, Response::Answer("yes".to_string()))
            .unwrap();
        for _ in 0..3 {
            tick(&mut runtime);
        }
        assert_eq!(runtime.position(target).0, 100.0);
        assert!(runtime.is_idle());
    }

    #[test]
    fn procedures_preserve_argument_order_and_resume_after_frontend_calls() {
        let project = Project::load_json(&json!({"targets": [{"name": "Sprite", "blocks": {
            "definition": {"opcode": "procedures_definition", "inputs": {"custom_block": [1, "prototype"]}, "next": "setx", "topLevel": true},
            "prototype": {"opcode": "procedures_prototype", "mutation": {
                "proccode": "position %s %s", "argumentids": "[\"z-id\",\"a-id\"]",
                "argumentnames": "[\"x\",\"y\"]", "argumentdefaults": "[0,0]", "warp": "true"
            }},
            "setx": {"opcode": "motion_setx", "inputs": {"X": [2, "x"]}, "next": "sety"},
            "sety": {"opcode": "motion_sety", "inputs": {"Y": [2, "y"]}},
            "x": {"opcode": "argument_reporter_string_number", "fields": {"VALUE": ["x", null]}},
            "y": {"opcode": "argument_reporter_string_number", "fields": {"VALUE": ["y", null]}}
        }}]})).unwrap();
        let target = *project.targets.keys().next().unwrap();
        let mut runtime = Runtime::new(project);
        runtime.run_instructions(&target, vec![Instruction::ProceduresCall {
            mutation: json!({"proccode": "position %s %s", "argumentids": "[\"z-id\",\"a-id\"]"}),
            arguments: std::collections::HashMap::from([
                ("z-id".to_string(), Instruction::MouseX),
                ("a-id".to_string(), Instruction::MouseY),
            ]),
        }]).unwrap();
        tick(&mut runtime);
        let first = runtime.pending_requests().next().unwrap().0;
        assert!(matches!(
            runtime.pending_request(first).unwrap().request,
            Request::Sense(Sensor::MouseX)
        ));
        runtime
            .respond(first, Response::Value(Value::Number(25.0)))
            .unwrap();
        tick(&mut runtime);
        let second = runtime.pending_requests().next().unwrap().0;
        assert!(matches!(
            runtime.pending_request(second).unwrap().request,
            Request::Sense(Sensor::MouseY)
        ));
        runtime
            .respond(second, Response::Value(Value::Number(50.0)))
            .unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.position(target), (25.0, 50.0));
        assert!(runtime.is_idle());
    }

    #[test]
    fn clones_copy_local_data_and_delete_their_threads_without_deleting_the_source() {
        let project = Project::load_json(&json!({"targets": [{"name": "Sprite", "variables": {"local": ["local", 7]}, "blocks": {
            "clone": {"opcode": "control_start_as_clone", "topLevel": true, "next": "set"},
            "set": {"opcode": "data_setvariableto", "fields": {"VARIABLE": ["local", "local"]}, "inputs": {"VALUE": [1, [4, 99]]}, "next": "wait"},
            "wait": {"opcode": "control_wait", "inputs": {"DURATION": [1, [4, 1]]}, "next": "delete"},
            "delete": {"opcode": "control_delete_this_clone"}
        }}]})).unwrap();
        let target = *project.targets.keys().next().unwrap();
        let mut runtime = Runtime::new(project);
        runtime
            .run_instructions(
                &target,
                vec![Instruction::CreateCloneOf {
                    clone: text("_myself_"),
                }],
            )
            .unwrap();
        tick(&mut runtime);
        tick(&mut runtime);
        let clone = *runtime
            .project
            .targets
            .keys()
            .find(|id| **id != target)
            .unwrap();
        assert_eq!(
            runtime.scratch.targets[&clone].variables["local"].value,
            Value::Number(99.0)
        );
        assert_eq!(
            runtime.scratch.targets[&target].variables["local"].value,
            Value::Number(7.0)
        );
        ready(runtime.tick_with_delta(Duration::from_secs(1))).unwrap();
        assert!(!runtime.project.targets.contains_key(&clone));
        assert!(!runtime.scratch.targets.contains_key(&clone));
        assert!(runtime.project.targets.contains_key(&target));
        assert!(runtime.is_idle());
    }

    #[test]
    fn global_variables_resolve_to_the_stage_and_project_settings_are_loaded() {
        let project = Project::load_json(&json!({"targets": [
            {"name": "Stage", "isStage": true, "tempo": 120, "videoState": "on-flipped", "variables": {"global": ["score", 10]}, "blocks": {}},
            {"name": "Sprite", "variables": {"local": ["score", 2]}, "blocks": {}}
        ]})).unwrap();
        let mut runtime = Runtime::new(project);
        let stage = runtime.stage_id().unwrap();
        let sprite = runtime.find_target("Sprite").unwrap();
        runtime
            .run_instructions(
                &sprite,
                vec![Instruction::ChangeVariableBy {
                    variable: Field {
                        value: Value::String("score".to_string()),
                        id: Some("global".to_string()),
                    },
                    value: number(5.0),
                }],
            )
            .unwrap();
        tick(&mut runtime);
        assert_eq!(
            runtime.scratch.targets[&stage].variables["global"].value,
            Value::Number(15.0)
        );
        assert_eq!(
            runtime.scratch.targets[&sprite].variables["local"].value,
            Value::Number(2.0)
        );
        assert_eq!(runtime.scratch.tempo, 120.0);
        assert_eq!(runtime.scratch.video_state, "on-flipped");
    }

    #[test]
    fn intermediate_transforms_are_preserved_in_effect_snapshots() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::PenDown,
                    Instruction::GotoXY {
                        x: number(10.0),
                        y: number(20.0),
                    },
                    Instruction::GotoXY {
                        x: number(30.0),
                        y: number(40.0),
                    },
                ],
            )
            .unwrap();
        tick(&mut runtime);
        let transforms: Vec<_> = runtime
            .drain_effects()
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::TargetChanged {
                    state: TargetSnapshot::Sprite(state),
                    ..
                } => Some((state.x, state.y)),
                _ => None,
            })
            .collect();
        assert_eq!(transforms, vec![(10.0, 20.0), (30.0, 40.0)]);
    }

    #[test]
    fn an_old_timed_bubble_does_not_clear_a_newer_bubble() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![Instruction::SayForSecs {
                    message: text("old"),
                    secs: number(1.0),
                }],
            )
            .unwrap();
        tick(&mut runtime);
        runtime
            .run_instructions(
                &target,
                vec![Instruction::Say {
                    message: text("new"),
                }],
            )
            .unwrap();
        tick(&mut runtime);
        runtime.drain_effects();
        ready(runtime.tick_with_delta(Duration::from_secs(1))).unwrap();
        assert!(runtime.drain_effects().is_empty());
    }

    #[test]
    fn stop_all_cancels_other_scripts_and_pending_requests_in_the_same_tick() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![Instruction::AskAndWait {
                    question: text("waiting"),
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
        runtime
            .run_instructions(&target, vec![Instruction::SetX { x: number(999.0) }])
            .unwrap();
        tick(&mut runtime);
        assert!(runtime.is_idle());
        assert_eq!(runtime.pending_requests().count(), 0);
        assert_eq!(runtime.position(target).0, 0.0);
        assert!(
            runtime
                .drain_effects()
                .iter()
                .any(|effect| matches!(effect, Effect::CancelRequest { .. }))
        );
    }

    #[test]
    fn sound_completion_and_stop_all_sounds_release_waiters() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::PlaySoundUntilDone {
                        sound: text("meow"),
                    },
                    Instruction::SetX { x: number(12.0) },
                ],
            )
            .unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.position(target).0, 0.0);
        runtime
            .run_instructions(&target, vec![Instruction::StopAllSounds])
            .unwrap();
        tick(&mut runtime);
        tick(&mut runtime);
        assert_eq!(runtime.position(target).0, 12.0);
        assert!(runtime.is_idle());
    }

    #[test]
    fn missing_sounds_are_no_ops_not_failed_frontend_requests() {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::PlaySound {
                        sound: text("not a sound"),
                    },
                    Instruction::PlaySoundUntilDone {
                        sound: text("not a sound"),
                    },
                    Instruction::IncrCounter,
                ],
            )
            .unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.scratch.counter, 1.0);
        assert!(runtime.is_idle());
        assert!(runtime.drain_effects().is_empty());
    }

    #[test]
    fn sounds_resolve_names_before_indices_only_for_string_inputs() {
        let project = Project::load_json(
            &json!({"targets": [{"name": "Sprite", "blocks": {}, "sounds": [
                {"name": "2"}, {"name": "meow"}, {"name": "woof"}
            ]}]}),
        )
        .unwrap();
        let target = *project.targets.keys().next().unwrap();
        let mut runtime = Runtime::new(project);
        let values = [
            Value::String("2".to_string()),
            Value::Number(2.0),
            Value::String("3abc".to_string()),
            Value::Number(0.0),
            Value::Number(-1.0),
        ];
        runtime
            .run_instructions(
                &target,
                values
                    .into_iter()
                    .map(|value| Instruction::PlaySound {
                        sound: Box::new(Instruction::Literal(value)),
                    })
                    .collect(),
            )
            .unwrap();
        tick(&mut runtime);
        let names: Vec<_> = runtime
            .drain_effects()
            .into_iter()
            .map(|effect| match effect {
                Effect::PlaySound { sound, .. } => sound,
                _ => panic!("unexpected sound effect"),
            })
            .collect();
        assert_eq!(names, ["2", "meow", "woof", "woof", "meow"]);
    }

    #[test]
    fn warp_loops_are_still_bounded_by_the_task_budget() {
        let (mut runtime, target) = runtime();
        runtime.tasks_per_tick = 20;
        let mut running = VmThread::new(target, vec![Instruction::Forever { body: vec![] }]);
        running.warp_depth = 1; // Simulate entry into a warp custom block.
        let thread = running.id;
        runtime.scheduler.add_thread(running);
        for _ in 0..100 {
            tick(&mut runtime);
        }
        assert!(runtime.scheduler.thread(thread).unwrap().tasks.len() < 5);
    }

    #[test]
    fn example_project_runs_instead_of_panicking() {
        let json =
            serde_json::from_slice(include_bytes!("../../examples/dev/project.json")).unwrap();
        let project = Project::load_json(&json).unwrap();
        let mut runtime = Runtime::new(project);
        ready(runtime.run_hats(&crate::BlockOpcode::WhenFlagClicked)).unwrap();
        for _ in 0..3 {
            tick(&mut runtime);
        }
        let messages: Vec<_> = runtime
            .drain_effects()
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Bubble {
                    text: Some(text), ..
                } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(
            messages,
            vec!["Hello!", "Bleh", "Hello!", "Bleh", "Hello!", "Bleh"]
        );
    }

    #[test]
    fn backdrop_switch_waits_for_its_matching_hat() {
        let project = Project::load_json(&json!({"targets": [
            {"isStage": true, "name": "Stage", "costumes": [{"name": "first"}, {"name": "second"}], "blocks": {}},
            {"name": "Sprite", "blocks": {
                "hat": {"opcode": "event_whenbackdropswitchesto", "topLevel": true, "fields": {"BACKDROP": ["second", null]}, "next": "wait"},
                "wait": {"opcode": "control_wait", "inputs": {"DURATION": [1, [4, 1]]}}
            }}
        ]})).unwrap();
        let mut runtime = Runtime::new(project);
        let sprite = runtime.find_target("Sprite").unwrap();
        runtime
            .run_instructions(
                &sprite,
                vec![
                    Instruction::SwitchBackdropToAndWait {
                        backdrop: text("second"),
                    },
                    Instruction::SetX { x: number(10.0) },
                ],
            )
            .unwrap();
        tick(&mut runtime);
        tick(&mut runtime);
        assert_eq!(runtime.position(sprite).0, 0.0);
        assert_eq!(runtime.costume_name(runtime.stage_id().unwrap()), "second");
        ready(runtime.tick_with_delta(Duration::from_secs(1))).unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.position(sprite).0, 10.0);
        assert!(runtime.is_idle());
    }

    #[test]
    fn recursive_procedure_arguments_are_scoped_per_call() {
        let project = Project::load_json(&json!({"targets": [{"name": "Sprite", "blocks": {
            "definition": {"opcode": "procedures_definition", "inputs": {"custom_block": [1, "prototype"]}, "next": "if", "topLevel": true},
            "prototype": {"opcode": "procedures_prototype", "mutation": {
                "proccode": "count %s", "argumentids": "[\"n\"]", "argumentnames": "[\"n\"]", "argumentdefaults": "[0]", "warp": "true"
            }},
            "if": {"opcode": "control_if", "next": "say", "inputs": {"CONDITION": [2, "positive"], "SUBSTACK": [2, "call"]}},
            "positive": {"opcode": "operator_gt", "inputs": {"OPERAND1": [2, "n"], "OPERAND2": [1, [4, 0]]}},
            "n": {"opcode": "argument_reporter_string_number", "fields": {"VALUE": ["n", null]}},
            "call": {"opcode": "procedures_call", "mutation": {"proccode": "count %s", "argumentids": "[\"n\"]"}, "inputs": {"n": [2, "subtract"]}},
            "subtract": {"opcode": "operator_subtract", "inputs": {"NUM1": [2, "n"], "NUM2": [1, [4, 1]]}},
            "say": {"opcode": "looks_say", "inputs": {"MESSAGE": [2, "n"]}}
        }}]})).unwrap();
        let target = *project.targets.keys().next().unwrap();
        let mut runtime = Runtime::new(project);
        runtime
            .run_instructions(
                &target,
                vec![Instruction::ProceduresCall {
                    mutation: json!({"proccode": "count %s", "argumentids": "[\"n\"]"}),
                    arguments: std::collections::HashMap::from([(
                        "n".to_string(),
                        Instruction::Literal(Value::Number(3.0)),
                    )]),
                }],
            )
            .unwrap();
        tick(&mut runtime);
        let messages: Vec<_> = runtime
            .drain_effects()
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Bubble {
                    text: Some(text), ..
                } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(messages, vec!["0", "1", "2", "3"]);
        assert!(runtime.is_idle());
    }
}
