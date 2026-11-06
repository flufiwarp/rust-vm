use crate::runtime::{BlockOpcode, Effect, Runtime, RuntimeError, ThreadId};
use crate::{Field, Instruction, ScratchList, ScratchVariable, Target, TargetId, Value};

impl Runtime {
    pub(crate) fn sound_name(&self, target: TargetId, value: &Value) -> Option<String> {
        let sounds = &self.project.targets.get(&target)?.sprite().scratch.sounds;
        if sounds.is_empty() {
            return None;
        }
        let text = value.to_string();
        if let Value::String(name) = value
            && let Some(sound) = sounds.iter().find(|sound| &sound.name == name)
        {
            return Some(sound.name.clone());
        }
        // Scratch tries a base-10 parseInt after exact name lookup, not a full
        // numeric cast: "2.5" and "2abc" both select the second sound.
        let text = text.trim_start();
        let (sign, digits) = if let Some(text) = text.strip_prefix('-') {
            (-1.0, text)
        } else {
            (1.0, text.strip_prefix('+').unwrap_or(text))
        };
        let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
        let number = digits.parse::<f64>().ok()? * sign;
        if !number.is_finite() {
            return None;
        }
        let index = (number - 1.0).rem_euclid(sounds.len() as f64) as usize;
        Some(sounds[index].name.clone())
    }

    pub(crate) fn data_owner(
        &self,
        target: TargetId,
        field: &Field,
        is_list: bool,
    ) -> (TargetId, String) {
        let mut targets = vec![target];
        let mut stages: Vec<_> = self
            .project
            .targets
            .iter()
            .filter(|(_, t)| matches!(t, Target::Stage { .. }))
            .map(|(id, _)| *id)
            .collect();
        stages.sort();
        targets.extend(stages);
        for owner in targets {
            let Some(data) = self.scratch.targets.get(&owner) else {
                continue;
            };
            if let Some(id) = &field.id {
                if (is_list && data.lists.contains_key(id))
                    || (!is_list && data.variables.contains_key(id))
                {
                    return (owner, id.clone());
                }
            } else if is_list {
                if let Some((id, _)) = data
                    .lists
                    .iter()
                    .find(|(_, list)| list.name == field.value.to_string())
                {
                    return (owner, id.clone());
                }
            } else if let Some((id, _)) = data
                .variables
                .iter()
                .find(|(_, variable)| variable.name == field.value.to_string())
            {
                return (owner, id.clone());
            }
        }
        (
            target,
            field.id.clone().unwrap_or_else(|| field.value.to_string()),
        )
    }

    pub(crate) fn variable(&self, target: TargetId, field: &Field) -> Value {
        let (owner, id) = self.data_owner(target, field, false);
        self.scratch
            .targets
            .get(&owner)
            .and_then(|data| data.variables.get(&id))
            .map(|variable| variable.value.clone())
            .unwrap_or(Value::Number(0.0))
    }

    pub(crate) fn set_variable(&mut self, target: TargetId, field: &Field, value: Value) {
        let (owner, id) = self.data_owner(target, field, false);
        self.scratch
            .targets
            .entry(owner)
            .or_default()
            .variables
            .insert(
                id.clone(),
                ScratchVariable {
                    name: field.value.to_string(),
                    value,
                },
            );
        self.effects.push_back(Effect::DataChanged {
            target_id: owner,
            id,
            is_list: false,
        });
    }

    pub(crate) fn list_instruction(
        &mut self,
        target: TargetId,
        instruction: &Instruction,
        args: &[Value],
    ) -> Option<Value> {
        use Instruction::*;
        let field = match instruction {
            ListContents { list }
            | AddToList { list, .. }
            | DeleteOfList { list, .. }
            | DeleteAllOfList { list }
            | InsertAtList { list, .. }
            | ReplaceItemOfList { list, .. }
            | ItemOfList { list, .. }
            | ItemNumOfList { list, .. }
            | LengthOfList { list }
            | ListContainsItem { list, .. }
            | ShowList { list }
            | HideList { list } => list,
            _ => return None,
        };
        let (owner, id) = self.data_owner(target, field, true);
        if matches!(instruction, ShowList { .. } | HideList { .. }) {
            self.effects.push_back(Effect::Monitor {
                target_id: owner,
                id,
                is_list: true,
                visible: matches!(instruction, ShowList { .. }),
            });
            return Some(Value::default());
        }
        let list = self
            .scratch
            .targets
            .entry(owner)
            .or_default()
            .lists
            .entry(id.clone())
            .or_insert_with(|| ScratchList {
                name: field.value.to_string(),
                items: Vec::new(),
            });
        let value = |i: usize| args.get(i).cloned().unwrap_or_default();
        let mut changed = false;
        let result = match instruction {
            ListContents { .. } => {
                let items: Vec<_> = list.items.iter().map(ToString::to_string).collect();
                let separator = if items.iter().all(|item| item.chars().count() == 1) {
                    ""
                } else {
                    " "
                };
                Value::String(items.join(separator))
            }
            LengthOfList { .. } => Value::Number(list.items.len() as f64),
            ItemOfList { .. } => index(&value(0), list.items.len())
                .map(|index| list.items[index].clone())
                .unwrap_or_default(),
            ItemNumOfList { .. } => Value::Number(
                list.items
                    .iter()
                    .position(|item| item.compare(&value(0)).is_eq())
                    .map(|i| i + 1)
                    .unwrap_or(0) as f64,
            ),
            ListContainsItem { .. } => Value::Boolean(
                list.items
                    .iter()
                    .any(|item| item.compare(&value(0)).is_eq()),
            ),
            AddToList { .. } => {
                if list.items.len() < 200_000 {
                    list.items.push(value(0));
                    changed = true;
                }
                Value::default()
            }
            DeleteAllOfList { .. } => {
                list.items.clear();
                changed = true;
                Value::default()
            }
            DeleteOfList { .. } => {
                if value(0).to_string() == "all" {
                    list.items.clear();
                    changed = true;
                } else if let Some(index) = index(&value(0), list.items.len()) {
                    list.items.remove(index);
                    changed = true;
                }
                Value::default()
            }
            InsertAtList { .. } => {
                if list.items.len() < 200_000
                    && let Some(index) = index(&value(0), list.items.len() + 1)
                {
                    list.items.insert(index, value(1));
                    changed = true;
                }
                Value::default()
            }
            ReplaceItemOfList { .. } => {
                if let Some(index) = index(&value(0), list.items.len()) {
                    list.items[index] = value(1);
                    changed = true;
                }
                Value::default()
            }
            _ => Value::default(),
        };
        if changed {
            self.effects.push_back(Effect::DataChanged {
                target_id: owner,
                id,
                is_list: true,
            });
        }
        Some(result)
    }

    pub(crate) fn costume_index(&self, id: TargetId) -> usize {
        match self.project.targets.get(&id) {
            Some(Target::Stage { state, .. }) => state.current_backdrop,
            Some(Target::Normal { state, .. } | Target::Clone { state, .. }) => {
                state.current_costume
            }
            None => 0,
        }
    }

    pub(crate) fn costume_name(&self, id: TargetId) -> String {
        self.project
            .targets
            .get(&id)
            .and_then(|target| target.sprite().scratch.costumes.get(self.costume_index(id)))
            .map(|asset| asset.name.clone())
            .unwrap_or_default()
    }

    pub(crate) fn stage_id(&self) -> Option<TargetId> {
        self.find_target("_stage_")
    }

    pub(crate) fn switch_costume(
        &mut self,
        id: TargetId,
        value: &Value,
        backdrop: bool,
    ) -> Result<Vec<ThreadId>, RuntimeError> {
        let Some(target) = self.project.targets.get(&id) else {
            return Ok(Vec::new());
        };
        let costumes = &target.sprite().scratch.costumes;
        if costumes.is_empty() {
            return Ok(Vec::new());
        }
        let current = self.costume_index(id);
        let name = value.to_string();
        let index = if let Some(index) = costumes.iter().position(|costume| costume.name == name) {
            index
        } else if name == "next costume" || name == "next backdrop" {
            (current + 1) % costumes.len()
        } else if name == "previous backdrop" {
            (current + costumes.len() - 1) % costumes.len()
        } else if name == "random backdrop" {
            rand::random_range(0..costumes.len())
        } else if let Ok(number) = name.trim().parse::<f64>() {
            if !number.is_finite() {
                return Ok(Vec::new());
            }
            (number.round() - 1.0).rem_euclid(costumes.len() as f64) as usize
        } else {
            return Ok(Vec::new());
        };
        match self.project.targets.get_mut(&id) {
            Some(Target::Stage { state, .. }) => state.current_backdrop = index,
            Some(Target::Normal { state, .. } | Target::Clone { state, .. }) => {
                state.current_costume = index
            }
            None => {}
        }
        self.changed(id);
        if backdrop {
            let name = self.costume_name(id);
            self.matching_hats(&BlockOpcode::WhenBackdropSwitchesTo, |_, block| {
                block
                    .fields
                    .get("BACKDROP")
                    .is_some_and(|field| field.value.to_string() == name)
            })
        } else {
            Ok(Vec::new())
        }
    }

    pub(crate) fn property(&self, object: &str, property: &str) -> Value {
        let Some(id) = self.find_target(object) else {
            return Value::Number(0.0);
        };
        let normal = self.normal(id);
        match property {
            "x position" => Value::Number(self.position(id).0),
            "y position" => Value::Number(self.position(id).1),
            "direction" => Value::Number(normal.map(|s| s.direction).unwrap_or(90.0)),
            "costume #" | "backdrop #" => Value::Number((self.costume_index(id) + 1) as f64),
            "costume name" | "backdrop name" => Value::String(self.costume_name(id)),
            "size" => Value::Number(normal.map(|s| s.size).unwrap_or(100.0)),
            "volume" => Value::Number(self.volume(id)),
            _ => self
                .scratch
                .targets
                .get(&id)
                .and_then(|data| {
                    data.variables
                        .values()
                        .find(|variable| variable.name == property)
                })
                .map(|variable| variable.value.clone())
                .unwrap_or(Value::Number(0.0)),
        }
    }
}

fn index(value: &Value, length: usize) -> Option<usize> {
    if length == 0 {
        return None;
    }
    match value.to_string().as_str() {
        "last" => Some(length - 1),
        "random" | "any" => Some(rand::random_range(0..length)),
        _ => {
            let number = value.as_number().floor();
            if number >= 1.0 && number <= length as f64 {
                Some(number as usize - 1)
            } else {
                None
            }
        }
    }
}
