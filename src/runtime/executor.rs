use super::thread::{Pending, PendingKind, Task};
use super::{
    Effect, PendingRequest, Request, Response, Runtime, RuntimeError, ThreadStatus, VmThread,
};
use crate::{Instruction, Value, cfg_variants};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadFlowResult {
    Continue,
    Yield,
    Finished,
}

impl Runtime {
    pub(crate) fn step_thread(
        &mut self,
        thread: &mut VmThread,
    ) -> Result<ThreadFlowResult, RuntimeError> {
        #[cfg(feature = "scratch-addons-debugger")]
        if self.is_debugger_paused() {
            return Ok(ThreadFlowResult::Yield);
        }
        if !self.project.targets.contains_key(&thread.target_id) {
            return Err(RuntimeError::TargetNotFound(thread.target_id));
        }
        match &thread.status {
            ThreadStatus::Finished => return Ok(ThreadFlowResult::Finished),
            ThreadStatus::Sleeping { until } if self.clock < *until => {
                return Ok(ThreadFlowResult::Yield);
            }
            ThreadStatus::Joining { threads }
                if threads
                    .iter()
                    .any(|id| self.scheduler.thread(*id).is_some()) =>
            {
                return Ok(ThreadFlowResult::Yield);
            }
            ThreadStatus::Waiting { request_id } => {
                let id = *request_id;
                let request = self
                    .requests
                    .get_mut(&id)
                    .ok_or(RuntimeError::UnknownRequest(id))?;
                let Some(response) = request.response.take() else {
                    return Ok(ThreadFlowResult::Yield);
                };
                self.requests.remove(&id);
                let pending = thread.pending.take().ok_or(RuntimeError::NoStackFrame)?;
                if pending.id != id {
                    return Err(RuntimeError::InvalidResponse(id));
                }
                if let Response::Failed(message) = response {
                    return Err(RuntimeError::RequestFailed { id, message });
                }
                let value = self.resume_response(thread, pending.kind, response)?;
                if !pending.discard {
                    thread.values.push(value);
                }
            }
            _ => {}
        }
        thread.status = ThreadStatus::Running;
        thread.yielded = false;
        for _ in 0..self.tasks_per_tick.max(1) {
            let Some(task) = thread.tasks.pop() else {
                thread.status = ThreadStatus::Finished;
                return Ok(ThreadFlowResult::Finished);
            };
            cfg_variants! {
                match (task) {
                    Task::Eval { instruction, discard } => {
                        let operands = instruction.operands();
                        thread.tasks.push(Task::Apply { instruction, count: operands.len(), discard });
                        thread.tasks.extend(operands.into_iter().rev().map(|instruction| Task::Eval { instruction, discard: false }));
                    },
                    Task::Apply { instruction, count, discard } => {
                        let start = thread.values.len().checked_sub(count).ok_or(RuntimeError::NoStackFrame)?;
                        let arguments = thread.values.split_off(start);
                        if let Some(value) = self.apply(thread, instruction, arguments, discard)?
                            && !discard {
                            thread.values.push(value);
                        }
                    },
                    Task::Forever { body } => {
                        thread.tasks.push(Task::Forever { body: body.clone() });
                        thread.loop_yield();
                        thread.push_stack(body);
                    },
                    Task::Yield => { thread.yielded = true; },
                    #[cfg(feature = "scratch")] {
                        Task::Repeat { body, remaining } => {
                            if remaining >= 1.0 {
                                thread.tasks.push(Task::Repeat { body: body.clone(), remaining: remaining - 1.0 });
                                thread.loop_yield();
                                thread.push_stack(body);
                            }
                        },
                        Task::ForEach { variable, value, body, index } => {
                            // The limit is an expression, not a cached repeat count.
                            // It may itself suspend for a frontend response.
                            thread.tasks.push(Task::ForEachStep { variable, value: value.clone(), body, index });
                            thread.tasks.push(Task::Eval { instruction: *value, discard: false });
                        },
                        Task::ForEachStep { variable, value, body, index } => {
                            let limit = thread.values.pop().ok_or(RuntimeError::NoStackFrame)?.as_number();
                            if index < limit {
                                let index = index + 1.0;
                                self.set_variable(thread.target_id, &variable, Value::Number(index));
                                thread.tasks.push(Task::ForEach { variable, value, body: body.clone(), index });
                                thread.loop_yield();
                                thread.push_stack(body);
                            }
                        },
                        Task::EndProcedure { warp } => {
                            thread.arguments.pop();
                            if warp { thread.warp_depth = thread.warp_depth.saturating_sub(1); }
                        },
                        Task::ClearBubble { kind, generation } => {
                            if self.scratch.targets.get(&thread.target_id).is_some_and(|data| data.bubble_generation == generation) {
                                self.effects.push_back(Effect::Bubble { target_id: thread.target_id, kind, text: None });
                            }
                        },
                        Task::Glide { from, to, start, duration } => {
                            let elapsed = self.clock.saturating_sub(start).as_secs_f64();
                            let progress = if duration.is_zero() { 1.0 } else { (elapsed / duration.as_secs_f64()).min(1.0) };
                            self.set_position(thread.target_id, from.0 + (to.0 - from.0) * progress, from.1 + (to.1 - from.1) * progress);
                            if progress < 1.0 {
                                thread.tasks.push(Task::Glide { from, to, start, duration });
                                thread.yielded = true;
                            }
                        },
                    }
                }
            }
            if thread.status == ThreadStatus::Finished {
                return Ok(ThreadFlowResult::Finished);
            }
            #[cfg(feature = "scratch-addons-debugger")]
            if self.is_debugger_paused() {
                return Ok(ThreadFlowResult::Yield);
            }
            if thread.status != ThreadStatus::Running || thread.yielded {
                return Ok(ThreadFlowResult::Yield);
            }
        }
        Ok(ThreadFlowResult::Yield)
    }

    fn apply(
        &mut self,
        thread: &mut VmThread,
        instruction: Instruction,
        arguments: Vec<Value>,
        discard: bool,
    ) -> Result<Option<Value>, RuntimeError> {
        #[cfg(not(feature = "scratch"))]
        let _ = arguments;
        match instruction {
            Instruction::Literal(value) => Ok(Some(value)),
            Instruction::Request { request } => {
                self.send_request(thread, request, PendingKind::Value, discard);
                Ok(None)
            }
            Instruction::Yield => {
                thread.yielded = true;
                Ok(Some(Value::default()))
            }
            Instruction::Forever { body } => {
                thread.tasks.push(Task::Forever { body });
                Ok(Some(Value::default()))
            }
            #[cfg(feature = "scratch")]
            instruction => self.apply_scratch(thread, instruction, arguments, discard),
        }
    }

    pub(crate) fn send_request(
        &mut self,
        thread: &mut VmThread,
        request: Request,
        kind: PendingKind,
        discard: bool,
    ) {
        let id = self.next_request_id;
        self.next_request_id += 1;
        self.requests.insert(
            id,
            PendingRequest {
                target_id: thread.target_id,
                thread_id: thread.id,
                request: request.clone(),
                response: None,
            },
        );
        self.effects.push_back(Effect::Request {
            id,
            target_id: thread.target_id,
            thread_id: thread.id,
            request,
        });
        thread.pending = Some(Pending { id, kind, discard });
        thread.status = ThreadStatus::Waiting { request_id: id };
    }

    fn resume_response(
        &mut self,
        thread: &mut VmThread,
        kind: PendingKind,
        response: Response,
    ) -> Result<Value, RuntimeError> {
        #[cfg(not(feature = "scratch"))]
        let _ = thread;
        cfg_variants! {
            match (kind) {
                PendingKind::Value => Ok(match response { Response::Value(value) => value, _ => Value::default() }),
                #[cfg(feature = "scratch")] {
                    PendingKind::Answer => {
                        if let Response::Answer(answer) = response { self.scratch.answer = answer; }
                        Ok(Value::default())
                    },
                    PendingKind::Position(action) => {
                        if let Response::Position { x, y } = response { self.position_action(thread, action, (x, y)); }
                        Ok(Value::default())
                    },
                    PendingKind::Motion => {
                        if let Response::Motion { x, y, direction } = response {
                            self.set_position(thread.target_id, x, y);
                            self.set_direction(thread.target_id, direction);
                        }
                        Ok(Value::default())
                    },
                    PendingKind::Distance => {
                        let (x0, y0) = self.position(thread.target_id);
                        Ok(if let Response::Position { x, y } = response {
                            Value::Number((x - x0).hypot(y - y0))
                        } else { Value::Number(10000.0) })
                    },
                    PendingKind::Loud => {
                        Ok(Value::Boolean(matches!(response, Response::Value(value) if value.as_number() > 10.0)))
                    },
                }
            }
        }
    }
}

impl Instruction {
    /// Evaluation order is explicit and stable, including procedure arguments.
    fn operands(&self) -> Vec<Instruction> {
        #[cfg(feature = "scratch")]
        {
            use Instruction::*;
            macro_rules! args { ($($arg:ident),*) => { vec![$($arg.as_ref().clone()),*] }; }
            match self {
                MoveSteps { steps } => args![steps],
                TurnRight { degrees } | TurnLeft { degrees } => args![degrees],
                ScrollRight { distance } | ScrollUp { distance } => args![distance],
                Goto { to } | DistanceTo { to } => args![to],
                GotoXY { x, y } => args![x, y],
                GlideTo { secs, to } => args![secs, to],
                GlideSecsToXY { secs, x, y } => args![secs, x, y],
                PointInDirection { direction } => args![direction],
                PointTowards { towards } => args![towards],
                ChangeXBy { dx } => args![dx],
                SetX { x } => args![x],
                ChangeYBy { dy } => args![dy],
                SetY { y } => args![y],
                SayForSecs { message, secs } | ThinkForSecs { message, secs } => {
                    args![message, secs]
                }
                Say { message } | Think { message } => args![message],
                SwitchCostumeTo { costume } => args![costume],
                SwitchBackdropTo { backdrop } | SwitchBackdropToAndWait { backdrop } => {
                    args![backdrop]
                }
                ChangeSizeBy { change }
                | ChangeEffectBy { change, .. }
                | ChangeStretchBy { change } => args![change],
                SetStretchTo { stretch } => args![stretch],
                SetSizeTo { size } | PenChangeSizeBy { size } | PenSetSizeTo { size } => {
                    args![size]
                }
                SetEffectTo { value, .. }
                | ChangeSoundEffectBy { value, .. }
                | SetSoundEffectTo { value, .. }
                | WhenGreaterThan { value, .. }
                | SetVariableTo { value, .. }
                | ChangeVariableBy { value, .. } => args![value],
                GoForwardBackwardLayers { layers, .. } => args![layers],
                PlaySound { sound } | PlaySoundUntilDone { sound } => args![sound],
                ChangeVolumeBy { volume } | SetVolumeTo { volume } => args![volume],
                Broadcast { broadcast } | BroadcastAndWait { broadcast } => args![broadcast],
                Wait { duration } => args![duration],
                Repeat { times, .. } => args![times],
                // ForEach evaluates its bound in a resumable task on every
                // iteration; eager evaluation here would evaluate it twice.
                ForEach { .. } => Vec::new(),
                If { condition, .. }
                | IfElse { condition, .. }
                | WaitUntil { condition }
                | RepeatUntil { condition, .. }
                | While { condition, .. } => args![condition],
                CreateCloneOf { clone } => args![clone],
                TouchingObject { object } | Of { object, .. } | WhenTouchingObject { object } => {
                    args![object]
                }
                TouchingColor { color } | PenSetColor { color } => args![color],
                ColorIsTouchingColor { color, color2 } => args![color, color2],
                AskAndWait { question } => args![question],
                KeyPressed { key } => args![key],
                Add { num1, num2 }
                | Subtract { num1, num2 }
                | Multiply { num1, num2 }
                | Divide { num1, num2 }
                | Mod { num1, num2 } => args![num1, num2],
                Random { from, to } => args![from, to],
                GreaterThan { operand1, operand2 }
                | LessThan { operand1, operand2 }
                | Equals { operand1, operand2 }
                | And { operand1, operand2 }
                | Or { operand1, operand2 } => args![operand1, operand2],
                Not { operand } => args![operand],
                Join { string1, string2 } | Contains { string1, string2 } => {
                    args![string1, string2]
                }
                LetterOf { letter, string } => args![letter, string],
                Length { string } => args![string],
                Round { num } | MathOp { num, .. } => args![num],
                AddToList { item, .. }
                | ItemNumOfList { item, .. }
                | ListContainsItem { item, .. } => args![item],
                DeleteOfList { index, .. } | ItemOfList { index, .. } => args![index],
                InsertAtList { index, item, .. } | ReplaceItemOfList { index, item, .. } => {
                    args![index, item]
                }
                ProceduresCall {
                    mutation,
                    arguments,
                } => super::scratch::argument_keys(mutation, arguments)
                    .into_iter()
                    .map(|key| arguments[&key].clone())
                    .collect(),
                PenChangeColorParamBy { color_param, value }
                | PenSetColorParamTo { color_param, value } => args![color_param, value],
                PenSetShade { shade } | PenChangeShadeBy { shade } => args![shade],
                PenSetHue { hue } | PenChangeHueBy { hue } => args![hue],
                MusicPlayDrumForBeats { drum, beats }
                | MusicMidiPlayDrumForBeats { drum, beats } => args![drum, beats],
                MusicRestForBeats { beats } => args![beats],
                MusicPlayNoteForBeats { note, beats } => args![note, beats],
                MusicSetInstrument { instrument } | MusicMidiSetInstrument { instrument } => {
                    args![instrument]
                }
                MusicSetTempo { tempo } | MusicChangeTempo { tempo } => args![tempo],
                VideoSensingWhenMotionGreaterThan { reference } => args![reference],
                VideoSensingVideoOn { attribute, subject } => args![attribute, subject],
                VideoSensingVideoToggle { video_state } => args![video_state],
                VideoSensingSetVideoTransparency { transparency } => args![transparency],
                Text2SpeechSpeakAndWait { words } => args![words],
                Text2SpeechSetVoice { voice } => args![voice],
                Text2SpeechSetLanguage { language } => args![language],
                TranslateGetTranslate { words, language } => args![words, language],
                _ => Vec::new(),
            }
        }
        #[cfg(not(feature = "scratch"))]
        {
            Vec::new()
        }
    }
}
