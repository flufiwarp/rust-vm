use super::*;
use serde_json::{Value as Json, json};

fn project(blocks: Json) -> (Project, TargetId) {
    let project = Project::load_json(&json!({
        "targets": [{ "isStage": true, "blocks": blocks }]
    }))
    .unwrap();
    let target_id = *project.targets.keys().next().unwrap();
    (project, target_id)
}

fn compile(blocks: Json) -> Result<Vec<Instruction>, CompileError> {
    let (project, target) = project(blocks);
    compile_block(&project, &"start".to_string(), &target)
}

#[test]
fn reports_missing_targets_and_blocks() {
    let (project, target) = project(json!({}));
    let block_id = "missing".to_string();
    assert!(matches!(compile_block(&project, &block_id, &target),
        Err(CompileError::BlockNotFound(id)) if id == block_id));
    let missing = TargetId::new_v4();
    assert!(matches!(compile_block(&project, &block_id, &missing),
        Err(CompileError::TargetNotFound(id)) if id == missing));
}

#[test]
fn unknown_opcodes_return_errors() {
    assert!(
        matches!(compile(json!({"start": {"opcode": "unknown_extension_block"}})),
        Err(CompileError::UnsupportedOpcode(BlockOpcode::Other(opcode)))
            if opcode == "unknown_extension_block")
    );
}

#[test]
fn compiles_yield_without_scratch() {
    let mut sprite = Sprite::load_json(&json!({
        "blocks": {"start": {"opcode": "yield"}}
    }))
    .unwrap();
    sprite.blocks.get_mut("start").unwrap().opcode = BlockOpcode::Yield;
    let mut compiler = Compiler {
        sprite: &sprite,
        active: HashSet::new(),
    };
    assert_eq!(
        compiler.stack(&"start".to_string()).unwrap(),
        vec![Instruction::Yield]
    );
}

#[cfg(not(feature = "scratch"))]
#[test]
fn scratch_opcodes_are_unsupported_without_the_feature() {
    for opcode in [
        "motion_movesteps",
        "control_for_each",
        "control_while",
        "control_get_counter",
        "sensing_loud",
        "pen_setPenHueToNumber",
        "music_midiSetInstrument",
    ] {
        assert!(matches!(
            compile(json!({"start": {"opcode": opcode}})),
            Err(CompileError::UnsupportedOpcode(BlockOpcode::Other(_)))
        ));
    }
}

#[cfg(feature = "scratch")]
mod scratch {
    use super::*;

    fn number(value: f64) -> Box<Instruction> {
        Box::new(Instruction::Literal(Value::Number(value)))
    }

    fn reference(name: &str, id: &str) -> Field {
        Field {
            value: Value::String(name.to_string()),
            id: Some(id.to_string()),
        }
    }

    #[test]
    fn every_known_scratch_opcode_compiles_to_its_instruction() {
        // Independent input/field schemas exercise the serialized Scratch names,
        // not just direct construction of BlockOpcode variants.
        macro_rules! check_blocks {
            ($($opcode:literal => $variant:ident [$($input:literal),*] [$($field:literal),*];)*) => {
                $(
                    let block = json!({
                        "opcode": $opcode,
                        "inputs": { $( $input: [1, [4, 10]] ),* },
                        "fields": { $( $field: ["option", "id"] ),* },
                        "mutation": {"proccode": "test", "argumentids": "[]", "warp": "false"}
                    });
                    // All stack-shaped inputs can be omitted for empty bodies.
                    let mut blocks = json!({"start": block});
                    if $opcode == "procedures_definition" {
                        blocks["start"]["inputs"]["custom_block"] = json!([1, "prototype"]);
                        blocks["prototype"] = json!({
                            "opcode": "procedures_prototype",
                            "mutation": {"proccode": "test", "argumentids": "[]", "warp": "false"}
                        });
                    }
                    let result = compile(blocks).unwrap_or_else(|error| panic!("{}: {error}", $opcode));
                    assert_eq!(result.len(), 1, "{}", $opcode);
                    assert!(matches!(&result[0], Instruction::$variant { .. }), "{}: {:?}", $opcode, result);
                )*
            };
        }

        check_blocks! {
            "motion_movesteps" => MoveSteps ["STEPS"] [];
            "motion_turnright" => TurnRight ["DEGREES"] [];
            "motion_turnleft" => TurnLeft ["DEGREES"] [];
            "motion_goto" => Goto ["TO"] [];
            "motion_gotoxy" => GotoXY ["X", "Y"] [];
            "motion_glideto" => GlideTo ["SECS", "TO"] [];
            "motion_glidesecstoxy" => GlideSecsToXY ["SECS", "X", "Y"] [];
            "motion_pointindirection" => PointInDirection ["DIRECTION"] [];
            "motion_pointtowards" => PointTowards ["TOWARDS"] [];
            "motion_changexby" => ChangeXBy ["DX"] [];
            "motion_setx" => SetX ["X"] [];
            "motion_changeyby" => ChangeYBy ["DY"] [];
            "motion_sety" => SetY ["Y"] [];
            "motion_ifonedgebounce" => IfOnEdgeBounce [] [];
            "motion_setrotationstyle" => SetRotationStyle [] ["STYLE"];
            "motion_xposition" => XPosition [] [];
            "motion_yposition" => YPosition [] [];
            "motion_direction" => Direction [] [];
            "motion_scroll_right" => ScrollRight ["DISTANCE"] [];
            "motion_scroll_up" => ScrollUp ["DISTANCE"] [];
            "motion_align_scene" => AlignScene [] ["ALIGNMENT"];
            "motion_xscroll" => XScroll [] [];
            "motion_yscroll" => YScroll [] [];
            "looks_sayforsecs" => SayForSecs ["MESSAGE", "SECS"] [];
            "looks_say" => Say ["MESSAGE"] [];
            "looks_thinkforsecs" => ThinkForSecs ["MESSAGE", "SECS"] [];
            "looks_think" => Think ["MESSAGE"] [];
            "looks_switchcostumeto" => SwitchCostumeTo ["COSTUME"] [];
            "looks_nextcostume" => NextCostume [] [];
            "looks_switchbackdropto" => SwitchBackdropTo ["BACKDROP"] [];
            "looks_switchbackdroptoandwait" => SwitchBackdropToAndWait ["BACKDROP"] [];
            "looks_nextbackdrop" => NextBackdrop [] [];
            "looks_changesizeby" => ChangeSizeBy ["CHANGE"] [];
            "looks_setsizeto" => SetSizeTo ["SIZE"] [];
            "looks_changeeffectby" => ChangeEffectBy ["CHANGE"] ["EFFECT"];
            "looks_seteffectto" => SetEffectTo ["VALUE"] ["EFFECT"];
            "looks_cleargraphiceffects" => ClearGraphicEffects [] [];
            "looks_show" => Show [] [];
            "looks_hide" => Hide [] [];
            "looks_gotofrontback" => GoToFrontBack [] ["FRONT_BACK"];
            "looks_goforwardbackwardlayers" => GoForwardBackwardLayers ["NUM"] ["FORWARD_BACKWARD"];
            "looks_costumenumbername" => CostumeNumberName [] ["NUMBER_NAME"];
            "looks_backdropnumbername" => BackdropNumberName [] ["NUMBER_NAME"];
            "looks_size" => Size [] [];
            "looks_hideallsprites" => HideAllSprites [] [];
            "looks_changestretchby" => ChangeStretchBy ["CHANGE"] [];
            "looks_setstretchto" => SetStretchTo ["STRETCH"] [];
            "sound_playuntildone" => PlaySoundUntilDone ["SOUND_MENU"] [];
            "sound_play" => PlaySound ["SOUND_MENU"] [];
            "sound_stopallsounds" => StopAllSounds [] [];
            "sound_changeeffectby" => ChangeSoundEffectBy ["VALUE"] ["EFFECT"];
            "sound_seteffectto" => SetSoundEffectTo ["VALUE"] ["EFFECT"];
            "sound_cleareffects" => ClearSoundEffects [] [];
            "sound_changevolumeby" => ChangeVolumeBy ["VOLUME"] [];
            "sound_setvolumeto" => SetVolumeTo ["VOLUME"] [];
            "sound_volume" => Volume [] [];
            "sound_beats_menu" => SoundBeatsMenu [] ["BEATS"];
            "sound_effects_menu" => SoundEffectsMenu [] ["EFFECT"];
            "event_whenflagclicked" => WhenFlagClicked [] [];
            "event_whenkeypressed" => WhenKeyPressed [] ["KEY_OPTION"];
            "event_whenthisspriteclicked" => WhenThisSpriteClicked [] [];
            "event_whenstageclicked" => WhenStageClicked [] [];
            "event_whenbackdropswitchesto" => WhenBackdropSwitchesTo [] ["BACKDROP"];
            "event_whengreaterthan" => WhenGreaterThan ["VALUE"] ["WHENGREATERTHANMENU"];
            "event_whenbroadcastreceived" => WhenIReceive [] ["BROADCAST_OPTION"];
            "event_whentouchingobject" => WhenTouchingObject ["TOUCHINGOBJECTMENU"] [];
            "event_broadcast" => Broadcast ["BROADCAST_INPUT"] [];
            "event_broadcastandwait" => BroadcastAndWait ["BROADCAST_INPUT"] [];
            "control_wait" => Wait ["DURATION"] [];
            "control_repeat" => Repeat ["TIMES"] [];
            "control_forever" => Forever [] [];
            "control_if" => If [] [];
            "control_if_else" => IfElse [] [];
            "control_wait_until" => WaitUntil [] [];
            "control_repeat_until" => RepeatUntil [] [];
            "control_while" => While [] [];
            "control_for_each" => ForEach ["VALUE"] ["VARIABLE"];
            "control_stop" => Stop [] ["STOP_OPTION"];
            "control_start_as_clone" => StartAsClone [] [];
            "control_create_clone_of" => CreateCloneOf ["CLONE_OPTION"] [];
            "control_delete_this_clone" => DeleteThisClone [] [];
            "control_get_counter" => GetCounter [] [];
            "control_incr_counter" => IncrCounter [] [];
            "control_clear_counter" => ClearCounter [] [];
            "control_all_at_once" => AllAtOnce [] [];
            "sensing_touchingobject" => TouchingObject ["TOUCHINGOBJECTMENU"] [];
            "sensing_touchingcolor" => TouchingColor ["COLOR"] [];
            "sensing_coloristouchingcolor" => ColorIsTouchingColor ["COLOR", "COLOR2"] [];
            "sensing_distanceto" => DistanceTo ["DISTANCETOMENU"] [];
            "sensing_askandwait" => AskAndWait ["QUESTION"] [];
            "sensing_answer" => Answer [] [];
            "sensing_keypressed" => KeyPressed ["KEY_OPTION"] [];
            "sensing_mousedown" => MouseDown [] [];
            "sensing_mousex" => MouseX [] [];
            "sensing_mousey" => MouseY [] [];
            "sensing_setdragmode" => SetDragMode [] ["DRAG_MODE"];
            "sensing_loudness" => Loudness [] [];
            "sensing_loud" => Loud [] [];
            "sensing_timer" => Timer [] [];
            "sensing_resettimer" => ResetTimer [] [];
            "sensing_of" => Of ["OBJECT"] ["PROPERTY"];
            "sensing_current" => Current [] ["CURRENTMENU"];
            "sensing_dayssince2000" => DaysSince2000 [] [];
            "sensing_username" => Username [] [];
            "sensing_userid" => UserId [] [];
            "operator_add" => Add ["NUM1", "NUM2"] [];
            "operator_subtract" => Subtract ["NUM1", "NUM2"] [];
            "operator_multiply" => Multiply ["NUM1", "NUM2"] [];
            "operator_divide" => Divide ["NUM1", "NUM2"] [];
            "operator_random" => Random ["FROM", "TO"] [];
            "operator_gt" => GreaterThan ["OPERAND1", "OPERAND2"] [];
            "operator_lt" => LessThan ["OPERAND1", "OPERAND2"] [];
            "operator_equals" => Equals ["OPERAND1", "OPERAND2"] [];
            "operator_and" => And [] [];
            "operator_or" => Or [] [];
            "operator_not" => Not [] [];
            "operator_join" => Join ["STRING1", "STRING2"] [];
            "operator_letter_of" => LetterOf ["LETTER", "STRING"] [];
            "operator_length" => Length ["STRING"] [];
            "operator_contains" => Contains ["STRING1", "STRING2"] [];
            "operator_mod" => Mod ["NUM1", "NUM2"] [];
            "operator_round" => Round ["NUM"] [];
            "operator_mathop" => MathOp ["NUM"] ["OPERATOR"];
            "data_variable" => Variable [] ["VARIABLE"];
            "data_setvariableto" => SetVariableTo ["VALUE"] ["VARIABLE"];
            "data_changevariableby" => ChangeVariableBy ["VALUE"] ["VARIABLE"];
            "data_showvariable" => ShowVariable [] ["VARIABLE"];
            "data_hidevariable" => HideVariable [] ["VARIABLE"];
            "data_listcontents" => ListContents [] ["LIST"];
            "data_addtolist" => AddToList ["ITEM"] ["LIST"];
            "data_deleteoflist" => DeleteOfList ["INDEX"] ["LIST"];
            "data_deletealloflist" => DeleteAllOfList [] ["LIST"];
            "data_insertatlist" => InsertAtList ["INDEX", "ITEM"] ["LIST"];
            "data_replaceitemoflist" => ReplaceItemOfList ["INDEX", "ITEM"] ["LIST"];
            "data_itemoflist" => ItemOfList ["INDEX"] ["LIST"];
            "data_itemnumoflist" => ItemNumOfList ["ITEM"] ["LIST"];
            "data_lengthoflist" => LengthOfList [] ["LIST"];
            "data_listcontainsitem" => ListContainsItem ["ITEM"] ["LIST"];
            "data_showlist" => ShowList [] ["LIST"];
            "data_hidelist" => HideList [] ["LIST"];
            "procedures_definition" => ProceduresDefinition [] [];
            "procedures_prototype" => ProceduresPrototype [] [];
            "procedures_call" => ProceduresCall [] [];
            "argument_reporter_string_number" => ArgumentReporterStringNumber [] ["VALUE"];
            "argument_reporter_boolean" => ArgumentReporterBoolean [] ["VALUE"];
            "pen_clear" => PenClear [] [];
            "pen_stamp" => PenStamp [] [];
            "pen_penDown" => PenDown [] [];
            "pen_penUp" => PenUp [] [];
            "pen_setPenColorToColor" => PenSetColor ["COLOR"] [];
            "pen_changePenColorParamBy" => PenChangeColorParamBy ["COLOR_PARAM", "VALUE"] [];
            "pen_setPenColorParamTo" => PenSetColorParamTo ["COLOR_PARAM", "VALUE"] [];
            "pen_changePenSizeBy" => PenChangeSizeBy ["SIZE"] [];
            "pen_setPenSizeTo" => PenSetSizeTo ["SIZE"] [];
            "pen_setPenShadeToNumber" => PenSetShade ["SHADE"] [];
            "pen_changePenShadeBy" => PenChangeShadeBy ["SHADE"] [];
            "pen_setPenHueToNumber" => PenSetHue ["HUE"] [];
            "pen_changePenHueBy" => PenChangeHueBy ["HUE"] [];
            "music_playDrumForBeats" => MusicPlayDrumForBeats ["DRUM", "BEATS"] [];
            "music_restForBeats" => MusicRestForBeats ["BEATS"] [];
            "music_playNoteForBeats" => MusicPlayNoteForBeats ["NOTE", "BEATS"] [];
            "music_setInstrument" => MusicSetInstrument ["INSTRUMENT"] [];
            "music_setTempo" => MusicSetTempo ["TEMPO"] [];
            "music_changeTempo" => MusicChangeTempo ["TEMPO"] [];
            "music_getTempo" => MusicGetTempo [] [];
            "music_midiPlayDrumForBeats" => MusicMidiPlayDrumForBeats ["DRUM", "BEATS"] [];
            "music_midiSetInstrument" => MusicMidiSetInstrument ["INSTRUMENT"] [];
            "videoSensing_whenMotionGreaterThan" => VideoSensingWhenMotionGreaterThan ["REFERENCE"] [];
            "videoSensing_videoOn" => VideoSensingVideoOn ["ATTRIBUTE", "SUBJECT"] [];
            "videoSensing_videoToggle" => VideoSensingVideoToggle ["VIDEO_STATE"] [];
            "videoSensing_setVideoTransparency" => VideoSensingSetVideoTransparency ["TRANSPARENCY"] [];
            "text2speech_speakAndWait" => Text2SpeechSpeakAndWait ["WORDS"] [];
            "text2speech_setVoice" => Text2SpeechSetVoice ["VOICE"] [];
            "text2speech_setLanguage" => Text2SpeechSetLanguage ["LANGUAGE"] [];
            "translate_getTranslate" => TranslateGetTranslate ["WORDS", "LANGUAGE"] [];
            "translate_getViewerLanguage" => TranslateGetViewerLanguage [] [];
            "motion_goto_menu" => MotionGotoMenu [] ["TO"];
            "motion_glideto_menu" => MotionGlideToMenu [] ["TO"];
            "motion_pointtowards_menu" => MotionPointTowardsMenu [] ["TOWARDS"];
            "looks_costume" => LooksCostume [] ["COSTUME"];
            "looks_backdrops" => LooksBackdrops [] ["BACKDROP"];
            "sound_sounds_menu" => SoundSoundsMenu [] ["SOUND_MENU"];
            "event_broadcast_menu" => EventBroadcastMenu [] ["BROADCAST_OPTION"];
            "control_create_clone_of_menu" => ControlCreateCloneOfMenu [] ["CLONE_OPTION"];
            "sensing_touchingobjectmenu" => SensingTouchingObjectMenu [] ["TOUCHINGOBJECTMENU"];
            "sensing_distancetomenu" => SensingDistanceToMenu [] ["DISTANCETOMENU"];
            "sensing_keyoptions" => SensingKeyOptions [] ["KEY_OPTION"];
            "sensing_of_object_menu" => SensingOfObjectMenu [] ["OBJECT"];
            "pen_menu_colorParam" => PenMenuColorParam [] ["colorParam"];
            "pen_menu_colorParam" => PenMenuColorParam [] ["COLOR_PARAM"];
            "music_menu_DRUM" => MusicMenuDrum [] ["DRUM"];
            "music_menu_INSTRUMENT" => MusicMenuInstrument [] ["INSTRUMENT"];
            "note" => MusicNote [] ["NOTE"];
            "videoSensing_menu_ATTRIBUTE" => VideoSensingMenuAttribute [] ["ATTRIBUTE"];
            "videoSensing_menu_SUBJECT" => VideoSensingMenuSubject [] ["SUBJECT"];
            "videoSensing_menu_VIDEO_STATE" => VideoSensingMenuVideoState [] ["VIDEO_STATE"];
            "text2speech_menu_voices" => Text2SpeechMenuVoices [] ["voices"];
            "text2speech_menu_languages" => Text2SpeechMenuLanguages [] ["languages"];
            "translate_menu_languages" => TranslateMenuLanguages [] ["languages"];
        }
    }

    #[test]
    fn compiles_nested_reporters_and_next_in_order() {
        let instructions = compile(json!({
            "start": {"opcode": "motion_gotoxy", "next": "say", "inputs": {
                "X": [3, "add", [4, 0]], "Y": [1, [4, -20]]
            }},
            "add": {"opcode": "operator_add", "inputs": {
                "NUM1": [1, [4, 3]], "NUM2": [2, "x"]
            }, "next": "not-a-real-block"},
            "x": {"opcode": "motion_xposition"},
            "say": {"opcode": "looks_say", "inputs": {"MESSAGE": [1, [10, "hello"]]}}
        }))
        .unwrap();
        assert_eq!(
            instructions,
            vec![
                Instruction::GotoXY {
                    x: Box::new(Instruction::Add {
                        num1: number(3.0),
                        num2: Box::new(Instruction::XPosition)
                    }),
                    y: number(-20.0),
                },
                Instruction::Say {
                    message: Box::new(Instruction::Literal(Value::String("hello".to_string())))
                },
            ]
        );
    }

    #[test]
    fn compiles_nested_control_flow_and_both_branches() {
        let instructions = compile(json!({
            "start": {"opcode": "control_repeat", "next": "after", "inputs": {
                "TIMES": [1, [6, 5]], "SUBSTACK": [2, "if"]
            }},
            "if": {"opcode": "control_if_else", "inputs": {
                "CONDITION": [2, "not"], "SUBSTACK": [2, "show"], "SUBSTACK2": [2, "hide"]
            }},
            "not": {"opcode": "operator_not"},
            "show": {"opcode": "looks_show", "next": "bounce"},
            "bounce": {"opcode": "motion_ifonedgebounce"},
            "hide": {"opcode": "looks_hide"},
            "after": {"opcode": "control_forever", "inputs": {"SUBSTACK": [2, null]}}
        }))
        .unwrap();
        assert_eq!(
            instructions,
            vec![
                Instruction::Repeat {
                    times: number(5.0),
                    body: vec![Instruction::IfElse {
                        condition: Box::new(Instruction::Not {
                            operand: Box::new(Instruction::Literal(Value::Boolean(false)))
                        }),
                        body: vec![Instruction::Show, Instruction::IfOnEdgeBounce],
                        else_body: vec![Instruction::Hide],
                    }]
                },
                Instruction::Forever { body: vec![] },
            ]
        );
    }

    #[test]
    fn compiles_all_inline_primitives_and_preserves_reference_ids() {
        let cases = [
            (json!([4, "2.5"]), Instruction::Literal(Value::Number(2.5))),
            (json!([5, 3]), Instruction::Literal(Value::Number(3.0))),
            (json!([6, 4]), Instruction::Literal(Value::Number(4.0))),
            (json!([7, -5]), Instruction::Literal(Value::Number(-5.0))),
            (json!([8, 90]), Instruction::Literal(Value::Number(90.0))),
            (
                json!([9, "#abcdef"]),
                Instruction::Literal(Value::String("#abcdef".to_string())),
            ),
            (
                json!([10, "hello"]),
                Instruction::Literal(Value::String("hello".to_string())),
            ),
            (
                json!([11, "message", "broadcast-id"]),
                Instruction::EventBroadcastMenu {
                    broadcast: reference("message", "broadcast-id"),
                },
            ),
            (
                json!([12, "score", "variable-id"]),
                Instruction::Variable {
                    variable: reference("score", "variable-id"),
                },
            ),
            (
                json!([13, "items", "list-id"]),
                Instruction::ListContents {
                    list: reference("items", "list-id"),
                },
            ),
        ];
        for (primitive, expected) in cases {
            let result = compile(json!({
                "start": {"opcode": "looks_say", "inputs": {"MESSAGE": [1, primitive]}}
            }))
            .unwrap();
            assert_eq!(
                result,
                vec![Instruction::Say {
                    message: Box::new(expected)
                }]
            );
        }
    }

    #[test]
    fn compiles_compact_standalone_reporters() {
        assert_eq!(
            compile(json!({"start": [12, "score", "score-id", 0, 0]})).unwrap(),
            vec![Instruction::Variable {
                variable: reference("score", "score-id")
            }]
        );
        assert_eq!(
            compile(json!({"start": [13, "items", "list-id", 0, 0]})).unwrap(),
            vec![Instruction::ListContents {
                list: reference("items", "list-id")
            }]
        );
    }

    #[test]
    fn preserves_custom_block_signatures_and_argument_ids() {
        let mutation = json!({
            "tagName": "mutation", "proccode": "test %s %b",
            "argumentids": "[\"text-id\",\"bool-id\"]",
            "argumentnames": "[\"text\",\"flag\"]", "argumentdefaults": "[\"\",false]", "warp": "true"
        });
        let result = compile(json!({
            "start": {"opcode": "procedures_definition", "next": "call", "inputs": {
                "custom_block": [1, "prototype"]
            }},
            "prototype": {"opcode": "procedures_prototype", "mutation": mutation, "inputs": {
                "text-id": [1, "text"], "bool-id": [1, "flag"]
            }},
            "text": {"opcode": "argument_reporter_string_number", "fields": {"VALUE": ["text", null]}},
            "flag": {"opcode": "argument_reporter_boolean", "fields": {"VALUE": ["flag", null]}},
            "call": {"opcode": "procedures_call", "mutation": mutation, "inputs": {
                "text-id": [1, [10, "hi"]], "bool-id": [2, "condition"]
            }},
            "condition": {"opcode": "operator_not"}
        })).unwrap();
        let Instruction::ProceduresDefinition { prototype } = &result[0] else {
            panic!()
        };
        let Instruction::ProceduresPrototype {
            mutation: compiled,
            arguments,
        } = prototype.as_ref()
        else {
            panic!()
        };
        assert_eq!(compiled, &mutation);
        assert_eq!(
            arguments["text-id"],
            Instruction::ArgumentReporterStringNumber {
                name: Field {
                    value: Value::String("text".to_string()),
                    id: None
                }
            }
        );
        assert!(matches!(
            arguments["bool-id"],
            Instruction::ArgumentReporterBoolean { .. }
        ));
        let Instruction::ProceduresCall {
            mutation: compiled,
            arguments,
        } = &result[1]
        else {
            panic!()
        };
        assert_eq!(compiled, &mutation);
        assert_eq!(
            arguments["text-id"],
            Instruction::Literal(Value::String("hi".to_string()))
        );
        assert!(matches!(arguments["bool-id"], Instruction::Not { .. }));
    }

    #[test]
    fn compiles_extension_menu_shadows() {
        let result = compile(json!({
            "start": {"opcode": "music_playNoteForBeats", "inputs": {
                "NOTE": [1, "note"], "BEATS": [1, [4, 0.25]]
            }},
            "note": {"opcode": "note", "fields": {"NOTE": ["60", null]}}
        }))
        .unwrap();
        assert_eq!(
            result,
            vec![Instruction::MusicPlayNoteForBeats {
                note: Box::new(Instruction::MusicNote {
                    note: Field {
                        value: Value::String("60".to_string()),
                        id: None
                    }
                }),
                beats: number(0.25),
            }]
        );
    }

    #[test]
    fn empty_boolean_sockets_and_substacks_compile() {
        let expected = vec![Instruction::IfElse {
            condition: Box::new(Instruction::Literal(Value::Boolean(false))),
            body: vec![],
            else_body: vec![],
        }];
        assert_eq!(
            compile(json!({"start": {"opcode": "control_if_else"}})).unwrap(),
            expected
        );
        assert_eq!(
            compile(json!({"start": {"opcode": "control_if_else", "inputs": {
                "CONDITION": [2, null], "SUBSTACK": [2, null], "SUBSTACK2": [2, null]
            }}}))
            .unwrap(),
            expected
        );
    }

    #[test]
    fn shared_reporters_are_not_cycles() {
        let result = compile(json!({
            "start": {"opcode": "motion_gotoxy", "next": "second", "inputs": {
                "X": [2, "x"], "Y": [2, "x"]
            }},
            "second": {"opcode": "motion_setx", "inputs": {"X": [2, "x"]}},
            "x": {"opcode": "motion_xposition"}
        }))
        .unwrap();
        assert_eq!(
            result,
            vec![
                Instruction::GotoXY {
                    x: Box::new(Instruction::XPosition),
                    y: Box::new(Instruction::XPosition)
                },
                Instruction::SetX {
                    x: Box::new(Instruction::XPosition)
                },
            ]
        );
    }

    #[test]
    fn malformed_blocks_return_compile_errors() {
        assert!(
            matches!(compile(json!({"start": {"opcode": "motion_movesteps"}})),
            Err(CompileError::MissingInput { block, input }) if block == "start" && input == "STEPS")
        );
        assert!(
            matches!(compile(json!({"start": {"opcode": "data_variable"}})),
            Err(CompileError::MissingField { block, field }) if block == "start" && field == "VARIABLE")
        );
        assert!(matches!(
            compile(
                json!({"start": {"opcode": "control_forever", "inputs": {"SUBSTACK": [1, [4, 10]]}}})
            ),
            Err(CompileError::InvalidSubstack { .. })
        ));
        assert!(
            matches!(compile(json!({"start": {"opcode": "procedures_call"}})),
            Err(CompileError::MissingMutation(id)) if id == "start")
        );
        assert!(
            matches!(compile(json!({"start": {"opcode": "looks_show", "next": "missing"}})),
            Err(CompileError::BlockNotFound(id)) if id == "missing")
        );
        assert!(
            matches!(compile(json!({"start": {"opcode": "looks_say", "inputs": {"MESSAGE": [2, "missing"]}}})),
            Err(CompileError::BlockNotFound(id)) if id == "missing")
        );
    }

    #[test]
    fn rejects_next_reporter_and_substack_cycles() {
        for blocks in [
            json!({"start": {"opcode": "looks_show", "next": "start"}}),
            json!({"start": {"opcode": "operator_not", "inputs": {"OPERAND": [2, "start"]}}}),
            json!({"start": {"opcode": "control_forever", "inputs": {"SUBSTACK": [2, "start"]}}}),
            json!({
                "start": {"opcode": "looks_show", "next": "second"},
                "second": {"opcode": "looks_hide", "next": "start"}
            }),
        ] {
            assert!(matches!(compile(blocks), Err(CompileError::CyclicBlock(id)) if id == "start"));
        }
    }

    #[test]
    fn compiles_example_project_hats() {
        let json: Json =
            serde_json::from_slice(include_bytes!("../../../examples/dev/project.json")).unwrap();
        let project = Project::load_json(&json).unwrap();
        let mut compiled = 0;
        for (target_id, target) in &project.targets {
            for (_, block_id) in &target.sprite().top_levels {
                compile_block(&project, block_id, target_id).unwrap();
                compiled += 1;
            }
        }
        assert!(compiled > 0);
    }
}
