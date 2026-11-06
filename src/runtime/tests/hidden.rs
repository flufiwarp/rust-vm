use super::*;
use crate::{BlockOpcode, Field};

fn number(value: f64) -> Box<Instruction> {
    Box::new(Instruction::Literal(Value::Number(value)))
}

fn variable(name: &str) -> Field {
    Field {
        value: Value::String(name.to_string()),
        id: Some(name.to_string()),
    }
}

fn compiled(blocks: serde_json::Value) -> (Runtime, TargetId) {
    let project =
        Project::load_json(&json!({"targets": [{"name": "Sprite", "blocks": blocks}]})).unwrap();
    let target = *project.targets.keys().next().unwrap();
    let code = compile_block(&project, &"start".to_string(), &target).unwrap();
    let mut runtime = Runtime::new(project);
    runtime.run_instructions(&target, code).unwrap();
    (runtime, target)
}

#[test]
fn for_each_compiles_and_uses_one_based_indices_with_fractional_limits() {
    let (mut runtime, target) = compiled(json!({
        "start": {"opcode": "control_for_each", "fields": {"VARIABLE": ["i", "i"]}, "inputs": {
            "VALUE": [1, [4, 2.5]], "SUBSTACK": [2, "append"]
        }},
        "append": {"opcode": "data_addtolist", "fields": {"LIST": ["seen", "seen"]}, "inputs": {"ITEM": [1, [12, "i", "i"]]}}
    }));
    for index in 1..=3 {
        tick(&mut runtime);
        assert_eq!(
            runtime.scratch.targets[&target].variables["i"].value,
            Value::Number(index as f64)
        );
    }
    tick(&mut runtime);
    assert!(runtime.is_idle());
    assert_eq!(
        runtime.scratch.targets[&target].lists["seen"].items,
        vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]
    );
}

#[test]
fn for_each_rechecks_its_limit_and_keeps_a_private_loop_index() {
    let (mut runtime, target) = runtime();
    let index = variable("i");
    let limit = variable("limit");
    runtime
        .run_instructions(
            &target,
            vec![
                Instruction::SetVariableTo {
                    variable: limit.clone(),
                    value: number(5.0),
                },
                Instruction::ForEach {
                    variable: index.clone(),
                    value: Box::new(Instruction::Variable {
                        variable: limit.clone(),
                    }),
                    body: vec![
                        Instruction::IncrCounter,
                        Instruction::SetVariableTo {
                            variable: limit,
                            value: number(2.0),
                        },
                        Instruction::SetVariableTo {
                            variable: index,
                            value: number(-99.0),
                        },
                    ],
                },
            ],
        )
        .unwrap();
    for _ in 0..5 {
        tick(&mut runtime);
    }
    assert_eq!(runtime.scratch.counter, 2.0);
    assert_eq!(
        runtime.scratch.targets[&target].variables["i"].value,
        Value::Number(-99.0)
    );
    assert!(runtime.is_idle());
}

#[test]
fn nested_for_each_loops_keep_separate_indices_even_with_a_tiny_task_budget() {
    let (mut runtime, target) = runtime();
    runtime.tasks_per_tick = 1;
    let thread = runtime
        .run_instructions(
            &target,
            vec![Instruction::ForEach {
                variable: variable("i"),
                value: number(2.0),
                body: vec![Instruction::ForEach {
                    variable: variable("j"),
                    value: number(2.0),
                    body: vec![Instruction::AddToList {
                        list: variable("seen"),
                        item: Box::new(Instruction::Join {
                            string1: Box::new(Instruction::Variable {
                                variable: variable("i"),
                            }),
                            string2: Box::new(Instruction::Variable {
                                variable: variable("j"),
                            }),
                        }),
                    }],
                }],
            }],
        )
        .unwrap();
    for _ in 0..200 {
        tick(&mut runtime);
        if let Some(thread) = runtime.scheduler.thread(thread) {
            assert!(thread.values.len() < 5);
        }
        if runtime.is_idle() {
            break;
        }
    }
    assert!(runtime.is_idle());
    assert_eq!(
        runtime.scratch.targets[&target].lists["seen"].items,
        ["11", "12", "21", "22"].map(|value| Value::String(value.to_string()))
    );
}

#[test]
fn empty_hidden_loops_cooperate_without_growing_the_task_stack() {
    for instruction in [
        Instruction::ForEach {
            variable: variable("i"),
            value: number(f64::INFINITY),
            body: vec![],
        },
        Instruction::While {
            condition: Box::new(Instruction::Literal(Value::Boolean(true))),
            body: vec![],
        },
    ] {
        let (mut runtime, target) = runtime();
        let thread = runtime
            .run_instructions(&target, vec![instruction])
            .unwrap();
        for _ in 0..100 {
            tick(&mut runtime);
            let running = runtime.scheduler.thread(thread).unwrap();
            assert!(running.tasks.len() < 5);
            assert!(running.values.is_empty());
            runtime.drain_effects();
        }
        runtime.stop_thread(thread);
        assert!(runtime.is_idle());
    }
}

#[test]
fn for_each_skips_nonpositive_bounds_without_modifying_the_variable() {
    for limit in [0.0, -1.0, f64::NAN] {
        let (mut runtime, target) = runtime();
        runtime
            .run_instructions(
                &target,
                vec![
                    Instruction::SetVariableTo {
                        variable: variable("i"),
                        value: number(99.0),
                    },
                    Instruction::ForEach {
                        variable: variable("i"),
                        value: number(limit),
                        body: vec![Instruction::IncrCounter],
                    },
                ],
            )
            .unwrap();
        tick(&mut runtime);
        assert_eq!(runtime.scratch.counter, 0.0);
        assert_eq!(
            runtime.scratch.targets[&target].variables["i"].value,
            Value::Number(99.0)
        );
        assert!(runtime.is_idle());
    }
}

#[test]
fn for_each_can_suspend_in_its_bound_without_duplicate_requests() {
    let (mut runtime, target) = runtime();
    let thread = runtime
        .run_instructions(
            &target,
            vec![Instruction::ForEach {
                variable: variable("i"),
                value: Box::new(Instruction::MouseX),
                body: vec![Instruction::IncrCounter],
            }],
        )
        .unwrap();
    for completed in 0..=3 {
        tick(&mut runtime);
        let id = runtime.pending_requests().next().unwrap().0;
        assert!(matches!(
            runtime.pending_request(id).unwrap().request,
            Request::Sense(Sensor::MouseX)
        ));
        assert_eq!(runtime.scratch.counter, completed as f64);
        runtime.drain_effects();
        tick(&mut runtime);
        tick(&mut runtime);
        assert!(runtime.drain_effects().is_empty());
        assert!(runtime.scheduler.thread(thread).unwrap().tasks.len() < 5);
        runtime
            .respond(id, Response::Value(Value::Number(2.5)))
            .unwrap();
        tick(&mut runtime);
    }
    assert!(runtime.is_idle());
    assert_eq!(runtime.scratch.counter, 3.0);
}

#[test]
fn for_each_resolves_stage_variables_by_id() {
    let project = Project::load_json(&json!({"targets": [
        {"isStage": true, "name": "Stage", "variables": {"global": ["i", 42]}, "blocks": {}},
        {"name": "Sprite", "variables": {"local": ["i", 99]}, "blocks": {}}
    ]}))
    .unwrap();
    let mut runtime = Runtime::new(project);
    let stage = runtime.stage_id().unwrap();
    let sprite = runtime.find_target("Sprite").unwrap();
    runtime
        .run_instructions(
            &sprite,
            vec![Instruction::ForEach {
                variable: Field {
                    value: Value::String("i".to_string()),
                    id: Some("global".to_string()),
                },
                value: number(2.0),
                body: vec![],
            }],
        )
        .unwrap();
    for _ in 0..3 {
        tick(&mut runtime);
    }
    assert_eq!(
        runtime.scratch.targets[&stage].variables["global"].value,
        Value::Number(2.0)
    );
    assert_eq!(
        runtime.scratch.targets[&sprite].variables["local"].value,
        Value::Number(99.0)
    );
    assert!(runtime.is_idle());
}

#[test]
fn while_rechecks_its_condition_each_iteration() {
    let (mut runtime, _) = compiled(json!({
        "start": {"opcode": "control_clear_counter", "next": "loop"},
        "loop": {"opcode": "control_while", "inputs": {"CONDITION": [2, "less"], "SUBSTACK": [2, "increment"]}},
        "less": {"opcode": "operator_lt", "inputs": {"OPERAND1": [2, "counter"], "OPERAND2": [1, [4, 3]]}},
        "counter": {"opcode": "control_get_counter"},
        "increment": {"opcode": "control_incr_counter"}
    }));
    for count in 1..=3 {
        tick(&mut runtime);
        assert_eq!(runtime.scratch.counter, count as f64);
    }
    tick(&mut runtime);
    assert!(runtime.is_idle());
}

#[test]
fn while_with_an_empty_condition_skips_its_body() {
    let (mut runtime, _) = compiled(json!({
        "start": {"opcode": "control_while", "inputs": {"SUBSTACK": [2, "increment"]}},
        "increment": {"opcode": "control_incr_counter"}
    }));
    tick(&mut runtime);
    assert!(runtime.is_idle());
    assert_eq!(runtime.scratch.counter, 0.0);
}

#[test]
fn while_with_a_request_condition_resumes_and_terminates() {
    let (mut runtime, target) = runtime();
    runtime
        .run_instructions(
            &target,
            vec![Instruction::While {
                condition: Box::new(Instruction::MouseDown),
                body: vec![Instruction::IncrCounter],
            }],
        )
        .unwrap();
    tick(&mut runtime);
    let first = runtime.pending_requests().next().unwrap().0;
    runtime
        .respond(first, Response::Value(Value::Boolean(true)))
        .unwrap();
    tick(&mut runtime);
    assert_eq!(runtime.scratch.counter, 1.0);
    tick(&mut runtime);
    let second = runtime.pending_requests().next().unwrap().0;
    assert_ne!(first, second);
    runtime
        .respond(second, Response::Value(Value::Boolean(false)))
        .unwrap();
    tick(&mut runtime);
    assert!(runtime.is_idle());
}

#[test]
fn counter_is_shared_between_targets_and_is_not_reset_by_the_green_flag() {
    let project = Project::load_json(&json!({"targets": [
        {"name": "one", "blocks": {}}, {"name": "two", "blocks": {}}
    ]}))
    .unwrap();
    let mut runtime = Runtime::new(project);
    let one = runtime.find_target("one").unwrap();
    let two = runtime.find_target("two").unwrap();
    runtime
        .run_instructions(&one, vec![Instruction::IncrCounter])
        .unwrap();
    runtime
        .run_instructions(&two, vec![Instruction::IncrCounter])
        .unwrap();
    tick(&mut runtime);
    assert_eq!(runtime.scratch.counter, 2.0);
    ready(runtime.green_flag()).unwrap();
    assert_eq!(runtime.scratch.counter, 2.0);
    runtime
        .run_instructions(
            &two,
            vec![
                Instruction::SetVariableTo {
                    variable: variable("saved"),
                    value: Box::new(Instruction::GetCounter),
                },
                Instruction::ClearCounter,
            ],
        )
        .unwrap();
    tick(&mut runtime);
    assert_eq!(
        runtime.scratch.targets[&two].variables["saved"].value,
        Value::Number(2.0)
    );
    assert_eq!(runtime.scratch.counter, 0.0);
}

#[test]
fn all_at_once_is_a_legacy_wrapper_not_a_warp_block() {
    let (mut runtime, target) = runtime();
    let thread = runtime
        .run_instructions(
            &target,
            vec![Instruction::AllAtOnce {
                body: vec![Instruction::Repeat {
                    times: number(3.0),
                    body: vec![Instruction::IncrCounter],
                }],
            }],
        )
        .unwrap();
    tick(&mut runtime);
    assert_eq!(runtime.scratch.counter, 1.0);
    assert_eq!(runtime.scheduler.thread(thread).unwrap().warp_depth, 0);
    for _ in 0..3 {
        tick(&mut runtime);
    }
    assert!(runtime.is_idle());
    assert_eq!(runtime.scratch.counter, 3.0);
}

#[test]
fn loud_uses_the_loudness_request_and_a_strict_threshold_of_ten() {
    for (loudness, expected) in [(0.0, false), (10.0, false), (10.1, true), (100.0, true)] {
        let (mut runtime, target) = compiled(json!({
            "start": {"opcode": "data_setvariableto", "fields": {"VARIABLE": ["loud", "loud"]}, "inputs": {"VALUE": [2, "loud"]}},
            "loud": {"opcode": "sensing_loud"}
        }));
        tick(&mut runtime);
        let id = runtime.pending_requests().next().unwrap().0;
        assert_eq!(
            runtime.pending_request(id).unwrap().request,
            Request::Sense(Sensor::Loudness)
        );
        runtime
            .respond(id, Response::Value(Value::Number(loudness)))
            .unwrap();
        tick(&mut runtime);
        assert_eq!(
            runtime.scratch.targets[&target].variables["loud"].value,
            Value::Boolean(expected)
        );
        assert!(runtime.is_idle());
    }
}

#[test]
fn legacy_no_ops_do_not_change_the_sprite_or_send_frontend_requests() {
    let (mut runtime, target) = compiled(json!({
        "start": {"opcode": "looks_hideallsprites", "next": "scroll"},
        "scroll": {"opcode": "motion_scroll_right", "inputs": {"DISTANCE": [1, [4, 100]]}, "next": "stretch"},
        "stretch": {"opcode": "looks_setstretchto", "inputs": {"STRETCH": [1, [4, 200]]}, "next": "userid"},
        "userid": {"opcode": "sensing_userid", "next": "xscroll"},
        "xscroll": {"opcode": "motion_xscroll"}
    }));
    let original = runtime.normal(target).unwrap().clone();
    tick(&mut runtime);
    assert_eq!(runtime.normal(target).unwrap(), &original);
    assert!(runtime.drain_effects().is_empty());
    assert!(runtime.is_idle());
}

#[test]
fn legacy_pen_blocks_preserve_their_distinct_units_in_effects() {
    let (mut runtime, target) = compiled(json!({
        "start": {"opcode": "pen_setPenHueToNumber", "inputs": {"HUE": [1, [4, 150]]}, "next": "hue"},
        "hue": {"opcode": "pen_changePenHueBy", "inputs": {"HUE": [1, [4, -5]]}, "next": "shade"},
        "shade": {"opcode": "pen_setPenShadeToNumber", "inputs": {"SHADE": [1, [4, 75]]}, "next": "change"},
        "change": {"opcode": "pen_changePenShadeBy", "inputs": {"SHADE": [1, [4, 10]]}}
    }));
    tick(&mut runtime);
    assert_eq!(
        runtime.drain_effects(),
        vec![
            Effect::Pen {
                target_id: target,
                command: PenCommand::SetHue(150.0)
            },
            Effect::Pen {
                target_id: target,
                command: PenCommand::ChangeHue(-5.0)
            },
            Effect::Pen {
                target_id: target,
                command: PenCommand::SetShade(75.0)
            },
            Effect::Pen {
                target_id: target,
                command: PenCommand::ChangeShade(10.0)
            },
        ]
    );
}

#[test]
fn legacy_midi_blocks_normalize_to_modern_music_effects() {
    let (mut runtime, target) = compiled(json!({
        "start": {"opcode": "music_midiSetInstrument", "inputs": {"INSTRUMENT": [1, [4, 9]]}, "next": "drum"},
        "drum": {"opcode": "music_midiPlayDrumForBeats", "inputs": {"DRUM": [1, [4, 35]], "BEATS": [1, [4, 0.25]]}}
    }));
    tick(&mut runtime);
    assert_eq!(
        runtime.drain_effects(),
        vec![
            Effect::Music {
                target_id: target,
                command: MusicCommand::SetInstrument(17.0)
            },
            Effect::Music {
                target_id: target,
                command: MusicCommand::Drum {
                    drum: 2.0,
                    beats: 0.25,
                    tempo: 60.0
                }
            },
        ]
    );
    assert!(!runtime.is_idle());
    ready(runtime.tick_with_delta(Duration::from_millis(250))).unwrap();
    assert!(runtime.is_idle());
}

#[test]
fn touching_object_hat_can_be_compiled_and_dispatched_by_the_frontend() {
    let project = Project::load_json(&json!({"targets": [{"name": "Sprite", "blocks": {
        "hat": {"opcode": "event_whentouchingobject", "topLevel": true, "next": "increment", "inputs": {"TOUCHINGOBJECTMENU": [1, "menu"]}},
        "menu": {"opcode": "sensing_touchingobjectmenu", "fields": {"TOUCHINGOBJECTMENU": ["_edge_", null]}},
        "increment": {"opcode": "control_incr_counter"}
    }}]})).unwrap();
    let target = *project.targets.keys().next().unwrap();
    let code = compile_block(&project, &"hat".to_string(), &target).unwrap();
    assert!(
        matches!(&code[0], Instruction::WhenTouchingObject { object } if matches!(object.as_ref(), Instruction::SensingTouchingObjectMenu { .. }))
    );
    let mut runtime = Runtime::new(project);
    ready(runtime.run_target_hats(&target, &BlockOpcode::WhenTouchingObject)).unwrap();
    tick(&mut runtime);
    assert_eq!(runtime.scratch.counter, 1.0);
}
