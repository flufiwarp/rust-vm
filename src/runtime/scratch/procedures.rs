use crate::runtime::thread::Task;
use crate::runtime::{Effect, Runtime, RuntimeError, ThreadId, VmThread, compile_block};
use crate::{Block, BlockOpcode, InputValue, Instruction, Target, TargetId, Value};
use std::collections::HashMap;

impl Runtime {
    pub async fn green_flag(&mut self) -> Result<(), RuntimeError> {
        self.stop_all();
        let clones: Vec<_> = self
            .project
            .targets
            .iter()
            .filter(|(_, target)| matches!(target, Target::Clone { .. }))
            .map(|(id, _)| *id)
            .collect();
        for target_id in clones {
            self.project.targets.remove(&target_id);
            self.scratch.targets.remove(&target_id);
            self.effects.push_back(Effect::CloneDeleted { target_id });
        }
        self.scratch.timer_started = self.clock;
        #[cfg(feature = "scratch-addons-debugger")]
        self.debugger.timers.clear();
        self.run_hats(&BlockOpcode::WhenFlagClicked).await
    }

    pub async fn broadcast_message(&mut self, name: &str) -> Result<Vec<ThreadId>, RuntimeError> {
        self.broadcast(name)
    }

    pub(crate) fn matching_hats(
        &mut self,
        opcode: &BlockOpcode,
        predicate: impl Fn(TargetId, &Block) -> bool,
    ) -> Result<Vec<ThreadId>, RuntimeError> {
        let mut blocks: Vec<_> = self
            .project
            .targets
            .iter()
            .flat_map(|(id, target)| {
                target
                    .sprite()
                    .top_levels
                    .iter()
                    .filter(|(kind, _)| kind == opcode)
                    .filter_map(|(_, block)| {
                        target.sprite().blocks.get(block).map(|block| (*id, block))
                    })
                    .filter(|(id, block)| predicate(*id, block))
                    .map(|(id, block)| (id, block.id.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        blocks.sort();
        blocks
            .into_iter()
            .filter_map(|(target, block)| self.start_hat(&target, &block).transpose())
            .collect()
    }

    /// Frontends can dispatch a filtered keyboard event without running hats for
    /// unrelated keys. Click hats can use run_target_hats directly.
    pub async fn key_pressed(&mut self, key: &str) -> Result<(), RuntimeError> {
        self.matching_hats(&BlockOpcode::WhenKeyPressed, |_, block| {
            block.fields.get("KEY_OPTION").is_some_and(|field| {
                let option = field.value.to_string();
                option == "any" || option.eq_ignore_ascii_case(key)
            })
        })?;
        Ok(())
    }

    pub(crate) fn broadcast(&mut self, name: &str) -> Result<Vec<ThreadId>, RuntimeError> {
        self.matching_hats(&BlockOpcode::WhenIReceive, |_, block| {
            block
                .fields
                .get("BROADCAST_OPTION")
                .is_some_and(|field| field.value.to_string() == name)
        })
    }

    pub(crate) fn call_procedure(
        &mut self,
        thread: &mut VmThread,
        mutation: &serde_json::Value,
        arguments: &HashMap<String, Instruction>,
        values: Vec<Value>,
        discard: bool,
    ) -> Result<bool, RuntimeError> {
        #[cfg(not(feature = "scratch-addons-debugger"))]
        let _ = discard;
        let code = mutation
            .get("proccode")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                RuntimeError::InvalidInstruction("missing procedure code".to_string())
            })?;
        #[cfg(feature = "scratch-addons-debugger")]
        if let Some(deferred) =
            self.call_debugger_procedure(thread, code, mutation, arguments, &values, discard)
        {
            return Ok(deferred);
        }
        let target = self
            .project
            .targets
            .get(&thread.target_id)
            .ok_or(RuntimeError::TargetNotFound(thread.target_id))?;
        let sprite = target.sprite();
        let definition = sprite
            .blocks
            .values()
            .filter(|block| block.opcode == BlockOpcode::ProceduresDefinition)
            .filter_map(|block| {
                let InputValue::Block(prototype) = block.inputs.get("custom_block")?.value() else {
                    return None;
                };
                let prototype = sprite.blocks.get(prototype)?;
                if prototype.mutation.as_ref()?.get("proccode")?.as_str()? == code {
                    Some((block, prototype))
                } else {
                    None
                }
            })
            .min_by_key(|(block, _)| &block.id);
        let Some(definition) = definition else {
            self.effects.push_back(Effect::Warning {
                target_id: thread.target_id,
                thread_id: thread.id,
                message: format!(
                    "procedure {code:?} was not found; {}",
                    if self.config.error_on_missing_procedures {
                        "strict mode is enabled"
                    } else {
                        "skipping call"
                    }
                ),
            });
            return if self.config.error_on_missing_procedures {
                Err(RuntimeError::ProcedureNotFound(code.to_string()))
            } else {
                Ok(false)
            };
        };
        if thread.arguments.len() >= 1024 {
            return Err(RuntimeError::CallDepthExceeded);
        }
        let metadata = definition.1.mutation.as_ref().ok_or_else(|| {
            RuntimeError::InvalidInstruction("missing prototype mutation".to_string())
        })?;
        let ids = array(metadata.get("argumentids"))?;
        let names = array(metadata.get("argumentnames"))?;
        let defaults = array(metadata.get("argumentdefaults"))?;
        let warp = metadata
            .get("warp")
            .is_some_and(|warp| warp == true || warp == "true");
        let body = if let Some(next) = &definition.0.next {
            compile_block(&self.project, next, &thread.target_id)?
        } else {
            Vec::new()
        };
        let keys = argument_keys(mutation, arguments);
        let supplied: HashMap<_, _> = keys.into_iter().zip(values).collect();
        let mut frame = HashMap::new();
        for (index, id) in ids.iter().enumerate() {
            let Some(id) = id.as_str() else {
                continue;
            };
            let name = names
                .get(index)
                .and_then(serde_json::Value::as_str)
                .unwrap_or(id);
            let default = defaults
                .get(index)
                .and_then(Value::from_json)
                .unwrap_or(Value::Number(0.0));
            frame.insert(
                name.to_string(),
                supplied.get(id).cloned().unwrap_or(default),
            );
        }
        thread.arguments.push(frame);
        if warp {
            thread.warp_depth += 1;
        }
        thread.tasks.push(Task::EndProcedure { warp });
        thread.push_stack(body);
        Ok(false)
    }

    pub(crate) fn create_clone(
        &mut self,
        current: TargetId,
        name: &str,
    ) -> Result<(), RuntimeError> {
        if self
            .project
            .targets
            .values()
            .filter(|target| matches!(target, Target::Clone { .. }))
            .count()
            >= 300
        {
            return Ok(());
        }
        let source = if name == "_myself_" {
            Some(current)
        } else {
            self.find_target(name)
        };
        let Some(source) = source else {
            return Ok(());
        };
        let Some(Target::Normal { sprite, state } | Target::Clone { sprite, state }) =
            self.project.targets.get(&source)
        else {
            return Ok(());
        };
        let mut state = state.clone();
        state.is_clone = true;
        let clone = Target::Clone {
            sprite: sprite.clone(),
            state,
        };
        let id = TargetId::new_v4();
        let hats = clone.sprite().find_top_level(&BlockOpcode::StartAsClone);
        self.project.targets.insert(id, clone);
        let data = self
            .scratch
            .targets
            .get(&source)
            .cloned()
            .unwrap_or_default();
        self.scratch.targets.insert(id, data);
        if let Some(state) = self.snapshot(id) {
            self.effects.push_back(Effect::CloneCreated {
                source_id: source,
                target_id: id,
                state,
            });
        }
        for hat in hats {
            self.start_hat(&id, &hat)?;
        }
        Ok(())
    }

    pub(crate) fn stop_target_threads(&mut self, target: TargetId) {
        let ids: Vec<_> = self
            .scheduler
            .thread_ids()
            .into_iter()
            .filter(|id| {
                self.scheduler
                    .thread(*id)
                    .is_some_and(|thread| thread.target_id == target)
            })
            .collect();
        for id in ids {
            self.stop_thread(id);
        }
    }
}

pub(crate) fn argument_keys(
    mutation: &serde_json::Value,
    arguments: &HashMap<String, Instruction>,
) -> Vec<String> {
    let ids = array(mutation.get("argumentids")).unwrap_or_default();
    let mut keys = Vec::new();
    for id in ids.iter().filter_map(serde_json::Value::as_str) {
        if arguments.contains_key(id) && !keys.iter().any(|key| key == id) {
            keys.push(id.to_string());
        }
    }
    let mut rest: Vec<_> = arguments
        .keys()
        .filter(|key| !keys.contains(key))
        .cloned()
        .collect();
    rest.sort();
    keys.extend(rest);
    keys
}

fn array(value: Option<&serde_json::Value>) -> Result<Vec<serde_json::Value>, RuntimeError> {
    match value {
        None => Ok(Vec::new()),
        Some(serde_json::Value::Array(array)) => Ok(array.clone()),
        Some(serde_json::Value::String(text)) => serde_json::from_str(text).map_err(|error| {
            RuntimeError::InvalidInstruction(format!("invalid procedure metadata: {error}"))
        }),
        _ => Err(RuntimeError::InvalidInstruction(
            "procedure argument metadata must be an array".to_string(),
        )),
    }
}
