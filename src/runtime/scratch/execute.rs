use super::{duration, legacy, operators};
use crate::runtime::thread::{PendingKind, PositionAction, Task};
use crate::runtime::{
    BubbleKind, Effect, MusicCommand, PenCommand, Request as FrontendRequest, Runtime,
    RuntimeError, Sensor, ThreadStatus, VideoCommand, VmThread,
};
use crate::{Instruction, Target, Value};

impl Runtime {
    pub(crate) fn apply_scratch(
        &mut self,
        thread: &mut VmThread,
        instruction: Instruction,
        arguments: Vec<Value>,
        discard: bool,
    ) -> Result<Option<Value>, RuntimeError> {
        use Instruction::*;
        if let Some(value) = operators::evaluate(&instruction, &arguments) {
            return Ok(Some(value));
        }
        if let Some(value) = self.list_instruction(thread.target_id, &instruction, &arguments) {
            return Ok(Some(value));
        }
        let id = thread.target_id;
        let v = |i: usize| arguments.get(i).cloned().unwrap_or_default();
        let n = |i| v(i).as_number();
        let s = |i| v(i).to_string();
        let mut output = Value::default();
        macro_rules! request {
            ($request:expr, $kind:expr) => {{
                self.send_request(thread, $request, $kind, discard);
                return Ok(None);
            }};
        }
        macro_rules! sensor {
            ($sensor:expr) => {
                request!(FrontendRequest::Sense($sensor), PendingKind::Value)
            };
        }
        macro_rules! pen {
            ($command:expr) => {
                self.effects.push_back(Effect::Pen {
                    target_id: id,
                    command: $command,
                })
            };
        }
        macro_rules! music {
            ($command:expr) => {
                self.effects.push_back(Effect::Music {
                    target_id: id,
                    command: $command,
                })
            };
        }

        match &instruction {
            MoveSteps { .. } => {
                let (x, y) = self.position(id);
                let direction = self
                    .normal(id)
                    .map(|s| s.direction)
                    .unwrap_or(90.0)
                    .to_radians();
                self.set_position(id, x + n(0) * direction.sin(), y + n(0) * direction.cos());
            }
            TurnRight { .. } | TurnLeft { .. } => {
                let direction = self.normal(id).map(|s| s.direction).unwrap_or(90.0);
                self.set_direction(
                    id,
                    direction
                        + if matches!(instruction, TurnRight { .. }) {
                            n(0)
                        } else {
                            -n(0)
                        },
                );
            }
            Goto { .. } | GlideTo { .. } | PointTowards { .. } => {
                let (name, action) = match instruction {
                    GlideTo { .. } => (s(1), PositionAction::Glide(n(0))),
                    PointTowards { .. } => (s(0), PositionAction::Point),
                    _ => (s(0), PositionAction::Goto),
                };
                if name == "_mouse_" {
                    request!(
                        FrontendRequest::Position { target: name },
                        PendingKind::Position(action)
                    );
                }
                if let Some(destination) = self.destination(&name) {
                    self.position_action(thread, action, destination);
                }
            }
            GotoXY { .. } => self.set_position(id, n(0), n(1)),
            GlideSecsToXY { .. } => {
                self.position_action(thread, PositionAction::Glide(n(0)), (n(1), n(2)))
            }
            PointInDirection { .. } => self.set_direction(id, n(0)),
            ChangeXBy { .. } => {
                let (x, y) = self.position(id);
                self.set_position(id, x + n(0), y);
            }
            SetX { .. } => {
                let (_, y) = self.position(id);
                self.set_position(id, n(0), y);
            }
            ChangeYBy { .. } => {
                let (x, y) = self.position(id);
                self.set_position(id, x, y + n(0));
            }
            SetY { .. } => {
                let (x, _) = self.position(id);
                self.set_position(id, x, n(0));
            }
            IfOnEdgeBounce => {
                let (x, y) = self.position(id);
                let direction = self.normal(id).map(|s| s.direction).unwrap_or(90.0);
                // The frontend owns costume bounds/renderer geometry.
                request!(
                    FrontendRequest::Bounce { x, y, direction },
                    PendingKind::Motion
                );
            }
            SetRotationStyle { style } => self.set_rotation(id, &style.value.to_string()),
            XPosition => output = Value::Number(self.position(id).0),
            YPosition => output = Value::Number(self.position(id).1),
            Direction => {
                output = Value::Number(self.normal(id).map(|s| s.direction).unwrap_or(90.0))
            }

            Say { .. } | SayForSecs { .. } | Think { .. } | ThinkForSecs { .. } => {
                let kind = if matches!(instruction, Think { .. } | ThinkForSecs { .. }) {
                    BubbleKind::Think
                } else {
                    BubbleKind::Say
                };
                let data = self.scratch.targets.entry(id).or_default();
                data.bubble_generation = data.bubble_generation.wrapping_add(1);
                let generation = data.bubble_generation;
                self.effects.push_back(Effect::Bubble {
                    target_id: id,
                    kind,
                    text: Some(s(0)),
                });
                if matches!(instruction, SayForSecs { .. } | ThinkForSecs { .. }) {
                    thread.tasks.push(Task::ClearBubble { kind, generation });
                    self.sleep(thread, n(1));
                }
            }
            SwitchCostumeTo { .. } => {
                self.switch_costume(id, &v(0), false)?;
            }
            NextCostume => {
                self.switch_costume(id, &Value::String("next costume".to_string()), false)?;
            }
            SwitchBackdropTo { .. } | SwitchBackdropToAndWait { .. } | NextBackdrop => {
                if let Some(stage) = self.stage_id() {
                    let value = if matches!(instruction, NextBackdrop) {
                        Value::String("next backdrop".to_string())
                    } else {
                        v(0)
                    };
                    let threads = self.switch_costume(stage, &value, true)?;
                    if matches!(instruction, SwitchBackdropToAndWait { .. }) && !threads.is_empty()
                    {
                        thread.status = ThreadStatus::Joining { threads };
                    }
                }
            }
            ChangeSizeBy { .. } | SetSizeTo { .. } => {
                if let Some(state) = self.normal_mut(id) {
                    state.size = if matches!(instruction, ChangeSizeBy { .. }) {
                        state.size + n(0)
                    } else {
                        n(0)
                    }
                    .max(0.0);
                    self.changed(id);
                }
            }
            ChangeEffectBy { effect, .. } | SetEffectTo { effect, .. } => {
                let name = effect.value.to_string().to_lowercase();
                let effects = &mut self.scratch.targets.entry(id).or_default().graphic_effects;
                let value = if matches!(instruction, ChangeEffectBy { .. }) {
                    effects.get(&name).copied().unwrap_or(0.0) + n(0)
                } else {
                    n(0)
                };
                let value = match name.as_str() {
                    "ghost" => value.clamp(0.0, 100.0),
                    "brightness" => value.clamp(-100.0, 100.0),
                    "color" => value.rem_euclid(200.0),
                    _ => value,
                };
                effects.insert(name.clone(), value);
                self.effects.push_back(Effect::GraphicEffect {
                    target_id: id,
                    effect: name,
                    value,
                });
            }
            ClearGraphicEffects => {
                self.scratch
                    .targets
                    .entry(id)
                    .or_default()
                    .graphic_effects
                    .clear();
                self.effects
                    .push_back(Effect::ClearGraphicEffects { target_id: id });
            }
            Show | Hide => {
                if let Some(state) = self.normal_mut(id) {
                    state.visible = matches!(instruction, Show);
                    self.changed(id);
                }
            }
            GoToFrontBack { front_back } => {
                if self.normal(id).is_some() {
                    self.reorder(
                        id,
                        if front_back.value.to_string() == "front" {
                            i64::MAX
                        } else {
                            0
                        },
                    );
                }
            }
            GoForwardBackwardLayers {
                forward_backward, ..
            } => {
                let mut layers: Vec<_> = self
                    .project
                    .targets
                    .keys()
                    .copied()
                    .filter(|id| self.normal(*id).is_some())
                    .collect();
                layers
                    .sort_by_key(|id| (self.normal(*id).map(|s| s.layer_order).unwrap_or(0), *id));
                if let Some(index) = layers.iter().position(|target| *target == id) {
                    let change = n(0).round().clamp(i32::MIN as f64, i32::MAX as f64) as i64;
                    self.reorder(
                        id,
                        index as i64
                            + if forward_backward.value.to_string() == "forward" {
                                change
                            } else {
                                -change
                            },
                    );
                }
            }
            CostumeNumberName { number_name } | BackdropNumberName { number_name } => {
                let owner = if matches!(instruction, BackdropNumberName { .. }) {
                    self.stage_id().unwrap_or(id)
                } else {
                    id
                };
                output = if number_name.value.to_string() == "name" {
                    Value::String(self.costume_name(owner))
                } else {
                    Value::Number((self.costume_index(owner) + 1) as f64)
                };
            }
            Size => output = Value::Number(self.normal(id).map(|s| s.size).unwrap_or(100.0)),

            PlaySoundUntilDone { .. } => {
                if let Some(sound) = self.sound_name(id, &v(0)) {
                    request!(
                        FrontendRequest::PlaySoundUntilDone { sound },
                        PendingKind::Value
                    );
                }
            }
            PlaySound { .. } => {
                if let Some(sound) = self.sound_name(id, &v(0)) {
                    self.effects.push_back(Effect::PlaySound {
                        target_id: id,
                        sound,
                    });
                }
            }
            StopAllSounds => {
                self.effects.push_back(Effect::StopAllSounds);
                // A stopped sound has completed from the waiting script's view.
                for request in self.requests.values_mut() {
                    if matches!(request.request, FrontendRequest::PlaySoundUntilDone { .. })
                        && request.response.is_none()
                    {
                        request.response = Some(crate::Response::Completed);
                    }
                }
            }
            ChangeSoundEffectBy { effect, .. } | SetSoundEffectTo { effect, .. } => {
                let name = effect.value.to_string().to_lowercase();
                let effects = &mut self.scratch.targets.entry(id).or_default().sound_effects;
                let value = if matches!(instruction, ChangeSoundEffectBy { .. }) {
                    effects.get(&name).copied().unwrap_or(0.0) + n(0)
                } else {
                    n(0)
                };
                let value = if name == "pan" {
                    value.clamp(-100.0, 100.0)
                } else {
                    value.clamp(-360.0, 360.0)
                };
                effects.insert(name.clone(), value);
                self.effects.push_back(Effect::SoundEffect {
                    target_id: id,
                    effect: name,
                    value,
                });
            }
            ClearSoundEffects => {
                self.scratch
                    .targets
                    .entry(id)
                    .or_default()
                    .sound_effects
                    .clear();
                self.effects
                    .push_back(Effect::ClearSoundEffects { target_id: id });
            }
            ChangeVolumeBy { .. } => self.set_volume(id, self.volume(id) + n(0)),
            SetVolumeTo { .. } => self.set_volume(id, n(0)),
            Volume => output = Value::Number(self.volume(id)),

            WhenFlagClicked
            | WhenKeyPressed { .. }
            | WhenThisSpriteClicked
            | WhenStageClicked
            | WhenBackdropSwitchesTo { .. }
            | WhenGreaterThan { .. }
            | WhenIReceive { .. }
            | WhenTouchingObject { .. }
            | StartAsClone
            | VideoSensingWhenMotionGreaterThan { .. } => {}
            Broadcast { .. } | BroadcastAndWait { .. } => {
                let threads = self.broadcast(&s(0))?;
                if matches!(instruction, BroadcastAndWait { .. }) && !threads.is_empty() {
                    thread.status = ThreadStatus::Joining { threads };
                }
            }

            Wait { .. } => self.sleep(thread, n(0)),
            Repeat { body, .. } => {
                thread.tasks.push(Task::Repeat {
                    body: body.clone(),
                    remaining: n(0).round().max(0.0),
                });
            }
            If { body, .. } => {
                if v(0).truthy() {
                    thread.push_stack(body.clone());
                }
            }
            IfElse {
                body, else_body, ..
            } => thread.push_stack(if v(0).truthy() {
                body.clone()
            } else {
                else_body.clone()
            }),
            WaitUntil { .. } => {
                if !v(0).truthy() {
                    thread.push_stack(vec![instruction.clone()]);
                    thread.yielded = true;
                }
            }
            RepeatUntil { body, .. } => {
                if !v(0).truthy() {
                    thread.push_stack(vec![instruction.clone()]);
                    thread.loop_yield();
                    thread.push_stack(body.clone());
                }
            }
            While { body, .. } => {
                if v(0).truthy() {
                    thread.push_stack(vec![instruction.clone()]);
                    thread.loop_yield();
                    thread.push_stack(body.clone());
                }
            }
            ForEach {
                variable,
                value,
                body,
            } => {
                thread.tasks.push(Task::ForEach {
                    variable: variable.clone(),
                    value: value.clone(),
                    body: body.clone(),
                    index: 0.0,
                });
            }
            Stop { option } => match option.value.to_string().as_str() {
                "all" => {
                    self.stop_all();
                    thread.status = ThreadStatus::Finished;
                }
                "other scripts in sprite" | "other scripts in stage" => {
                    self.stop_target_threads(id)
                }
                _ => thread.stop_this_script(),
            },
            CreateCloneOf { .. } => self.create_clone(id, &s(0))?,
            DeleteThisClone => {
                if matches!(self.project.targets.get(&id), Some(Target::Clone { .. })) {
                    self.stop_target_threads(id);
                    self.project.targets.remove(&id);
                    self.scratch.targets.remove(&id);
                    self.effects
                        .push_back(Effect::CloneDeleted { target_id: id });
                    thread.status = ThreadStatus::Finished;
                }
            }
            GetCounter => output = Value::Number(self.scratch.counter),
            IncrCounter => self.scratch.counter += 1.0,
            ClearCounter => self.scratch.counter = 0.0,
            AllAtOnce { body } => {
                // Scratch 2 compatibility: this is just a wrapper, not warp.
                thread.push_stack(body.clone());
            }

            TouchingObject { .. } => {
                sensor!(Sensor::TouchingObject(s(0)));
            }
            TouchingColor { .. } => {
                sensor!(Sensor::TouchingColor(s(0)));
            }
            ColorIsTouchingColor { .. } => {
                sensor!(Sensor::ColorIsTouchingColor {
                    color: s(0),
                    color2: s(1)
                });
            }
            DistanceTo { .. } => {
                if s(0) == "_mouse_" {
                    request!(
                        FrontendRequest::Position { target: s(0) },
                        PendingKind::Distance
                    );
                }
                output = Value::Number(if let Some(to) = self.destination(&s(0)) {
                    let from = self.position(id);
                    (to.0 - from.0).hypot(to.1 - from.1)
                } else {
                    10000.0
                });
            }
            AskAndWait { .. } => {
                request!(FrontendRequest::Ask { question: s(0) }, PendingKind::Answer);
            }
            Answer => output = Value::String(self.scratch.answer.clone()),
            KeyPressed { .. } => {
                sensor!(Sensor::KeyPressed(s(0)));
            }
            MouseDown => {
                sensor!(Sensor::MouseDown);
            }
            MouseX => {
                sensor!(Sensor::MouseX);
            }
            MouseY => {
                sensor!(Sensor::MouseY);
            }
            SetDragMode { mode } => {
                if let Some(state) = self.normal_mut(id) {
                    state.draggable = mode.value.to_string() == "draggable";
                    self.changed(id);
                }
            }
            Loudness => {
                sensor!(Sensor::Loudness);
            }
            Loud => {
                request!(FrontendRequest::Sense(Sensor::Loudness), PendingKind::Loud);
            }
            Timer => {
                output = Value::Number(
                    self.clock
                        .saturating_sub(self.scratch.timer_started)
                        .as_secs_f64(),
                )
            }
            ResetTimer => self.scratch.timer_started = self.clock,
            Of { property, .. } => output = self.property(&s(0), &property.value.to_string()),
            Current { current_menu } => {
                sensor!(Sensor::Current(current_menu.value.to_string()));
            }
            DaysSince2000 => {
                sensor!(Sensor::DaysSince2000);
            }
            Username => {
                sensor!(Sensor::Username);
            }
            // These obsolete blocks are intentionally no-ops in Scratch 3.
            UserId
            | ScrollRight { .. }
            | ScrollUp { .. }
            | AlignScene { .. }
            | XScroll
            | YScroll
            | HideAllSprites
            | ChangeStretchBy { .. }
            | SetStretchTo { .. } => {}

            Variable { variable } => output = self.variable(id, variable),
            SetVariableTo { variable, .. } => self.set_variable(id, variable, v(0)),
            ChangeVariableBy { variable, .. } => self.set_variable(
                id,
                variable,
                Value::Number(self.variable(id, variable).as_number() + n(0)),
            ),
            ShowVariable { variable } | HideVariable { variable } => {
                let (owner, key) = self.data_owner(id, variable, false);
                self.effects.push_back(Effect::Monitor {
                    target_id: owner,
                    id: key,
                    is_list: false,
                    visible: matches!(instruction, ShowVariable { .. }),
                });
            }
            ProceduresDefinition { .. } | ProceduresPrototype { .. } => {}
            ProceduresCall {
                mutation,
                arguments: inputs,
            } => {
                if self.call_procedure(thread, mutation, inputs, arguments.clone(), discard)? {
                    return Ok(None);
                }
            }
            ArgumentReporterStringNumber { name } | ArgumentReporterBoolean { name } => {
                output = thread
                    .arguments
                    .last()
                    .and_then(|args| args.get(&name.value.to_string()))
                    .cloned()
                    .unwrap_or({
                        if matches!(instruction, ArgumentReporterBoolean { .. }) {
                            Value::Boolean(false)
                        } else {
                            Value::Number(0.0)
                        }
                    });
            }

            PenClear => pen!(PenCommand::Clear),
            PenStamp => pen!(PenCommand::Stamp),
            PenDown => pen!(PenCommand::Down),
            PenUp => pen!(PenCommand::Up),
            PenSetColor { .. } => pen!(PenCommand::SetColor(s(0))),
            PenChangeColorParamBy { .. } => pen!(PenCommand::ChangeColorParam {
                parameter: s(0),
                value: n(1)
            }),
            PenSetColorParamTo { .. } => pen!(PenCommand::SetColorParam {
                parameter: s(0),
                value: n(1)
            }),
            PenChangeSizeBy { .. } => pen!(PenCommand::ChangeSize(n(0))),
            PenSetSizeTo { .. } => pen!(PenCommand::SetSize(n(0))),
            PenSetShade { .. } => pen!(PenCommand::SetShade(n(0))),
            PenChangeShadeBy { .. } => pen!(PenCommand::ChangeShade(n(0))),
            PenSetHue { .. } => pen!(PenCommand::SetHue(n(0))),
            PenChangeHueBy { .. } => pen!(PenCommand::ChangeHue(n(0))),
            MusicPlayDrumForBeats { .. }
            | MusicMidiPlayDrumForBeats { .. }
            | MusicPlayNoteForBeats { .. }
            | MusicRestForBeats { .. } => {
                let beats = if matches!(instruction, MusicRestForBeats { .. }) {
                    n(0)
                } else {
                    n(1)
                }
                .clamp(0.0, 100.0);
                let tempo = self.scratch.tempo;
                match instruction {
                    MusicPlayDrumForBeats { .. } => music!(MusicCommand::Drum {
                        drum: n(0),
                        beats,
                        tempo
                    }),
                    MusicMidiPlayDrumForBeats { .. } => music!(MusicCommand::Drum {
                        drum: legacy::midi_drum(n(0)),
                        beats,
                        tempo,
                    }),
                    MusicPlayNoteForBeats { .. } => music!(MusicCommand::Note {
                        note: n(0),
                        beats,
                        tempo
                    }),
                    _ => {}
                }
                self.sleep(thread, beats * 60.0 / tempo);
            }
            MusicSetInstrument { .. } => music!(MusicCommand::SetInstrument(n(0))),
            MusicMidiSetInstrument { .. } => {
                music!(MusicCommand::SetInstrument(legacy::midi_instrument(n(0))))
            }
            MusicSetTempo { .. } | MusicChangeTempo { .. } => {
                self.scratch.tempo = if matches!(instruction, MusicChangeTempo { .. }) {
                    self.scratch.tempo + n(0)
                } else {
                    n(0)
                }
                .clamp(20.0, 500.0);
                music!(MusicCommand::SetTempo(self.scratch.tempo));
            }
            MusicGetTempo => output = Value::Number(self.scratch.tempo),
            VideoSensingVideoOn { .. } => {
                sensor!(Sensor::Video {
                    attribute: s(0),
                    subject: s(1)
                });
            }
            VideoSensingVideoToggle { .. } => {
                self.scratch.video_state = s(0);
                self.effects
                    .push_back(Effect::Video(VideoCommand::Toggle(s(0))));
            }
            VideoSensingSetVideoTransparency { .. } => {
                self.scratch.video_transparency = n(0).clamp(0.0, 100.0);
                self.effects
                    .push_back(Effect::Video(VideoCommand::SetTransparency(
                        self.scratch.video_transparency,
                    )));
            }
            Text2SpeechSpeakAndWait { .. } => {
                request!(
                    FrontendRequest::Speak {
                        words: s(0),
                        voice: self.scratch.voice.clone(),
                        language: self.scratch.language.clone(),
                    },
                    PendingKind::Value
                );
            }
            Text2SpeechSetVoice { .. } => self.scratch.voice = s(0),
            Text2SpeechSetLanguage { .. } => self.scratch.language = s(0),
            TranslateGetTranslate { .. } => {
                request!(
                    FrontendRequest::Translate {
                        words: s(0),
                        language: s(1)
                    },
                    PendingKind::Value
                );
            }
            TranslateGetViewerLanguage => {
                sensor!(Sensor::ViewerLanguage);
            }

            MotionGotoMenu { to } | MotionGlideToMenu { to } | SensingDistanceToMenu { to } => {
                output = to.value.clone()
            }
            MotionPointTowardsMenu { towards } => output = towards.value.clone(),
            LooksCostume { costume } => output = costume.value.clone(),
            LooksBackdrops { backdrop } => output = backdrop.value.clone(),
            SoundSoundsMenu { sound } => output = sound.value.clone(),
            SoundBeatsMenu { beats } => output = beats.value.clone(),
            SoundEffectsMenu { effect } => output = effect.value.clone(),
            EventBroadcastMenu { broadcast } => output = broadcast.value.clone(),
            ControlCreateCloneOfMenu { clone } => output = clone.value.clone(),
            SensingTouchingObjectMenu { object } | SensingOfObjectMenu { object } => {
                output = object.value.clone()
            }
            SensingKeyOptions { key } => output = key.value.clone(),
            PenMenuColorParam { color_param } => output = color_param.value.clone(),
            MusicMenuDrum { drum } => output = drum.value.clone(),
            MusicMenuInstrument { instrument } => output = instrument.value.clone(),
            MusicNote { note } => output = note.value.clone(),
            VideoSensingMenuAttribute { attribute } => output = attribute.value.clone(),
            VideoSensingMenuSubject { subject } => output = subject.value.clone(),
            VideoSensingMenuVideoState { video_state } => output = video_state.value.clone(),
            Text2SpeechMenuVoices { voice } => output = voice.value.clone(),
            Text2SpeechMenuLanguages { language } | TranslateMenuLanguages { language } => {
                output = language.value.clone()
            }

            // Handled by the base runner or pure/data dispatch above. Keeping the
            // match exhaustive makes newly added instructions require a handler.
            Literal(_)
            | Request { .. }
            | Yield
            | Forever { .. }
            | Add { .. }
            | Subtract { .. }
            | Multiply { .. }
            | Divide { .. }
            | Random { .. }
            | GreaterThan { .. }
            | LessThan { .. }
            | Equals { .. }
            | And { .. }
            | Or { .. }
            | Not { .. }
            | Join { .. }
            | LetterOf { .. }
            | Length { .. }
            | Contains { .. }
            | Mod { .. }
            | Round { .. }
            | MathOp { .. }
            | ListContents { .. }
            | AddToList { .. }
            | DeleteOfList { .. }
            | DeleteAllOfList { .. }
            | InsertAtList { .. }
            | ReplaceItemOfList { .. }
            | ItemOfList { .. }
            | ItemNumOfList { .. }
            | LengthOfList { .. }
            | ListContainsItem { .. }
            | ShowList { .. }
            | HideList { .. } => {
                return Err(RuntimeError::InvalidInstruction(format!(
                    "unexpected dispatch: {instruction:?}"
                )));
            }
        }
        Ok(Some(output))
    }

    fn sleep(&mut self, thread: &mut VmThread, seconds: f64) {
        thread.status = ThreadStatus::Sleeping {
            until: self.clock.saturating_add(duration(seconds)),
        };
    }
}
