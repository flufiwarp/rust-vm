use super::duration;
use crate::runtime::thread::{PositionAction, Task};
use crate::runtime::{Effect, Runtime, TargetSnapshot, VmThread};
use crate::{NormalTargetState, RotationStyle, Target, TargetId};

impl Runtime {
    pub(crate) fn normal(&self, id: TargetId) -> Option<&NormalTargetState> {
        match self.project.targets.get(&id)? {
            Target::Normal { state, .. } | Target::Clone { state, .. } => Some(state),
            _ => None,
        }
    }
    pub(crate) fn normal_mut(&mut self, id: TargetId) -> Option<&mut NormalTargetState> {
        match self.project.targets.get_mut(&id)? {
            Target::Normal { state, .. } | Target::Clone { state, .. } => Some(state),
            _ => None,
        }
    }
    pub(crate) fn snapshot(&self, id: TargetId) -> Option<TargetSnapshot> {
        Some(match self.project.targets.get(&id)? {
            Target::Stage { state, .. } => TargetSnapshot::Stage(state.clone()),
            Target::Normal { state, .. } | Target::Clone { state, .. } => {
                TargetSnapshot::Sprite(state.clone())
            }
        })
    }
    pub(crate) fn changed(&mut self, id: TargetId) {
        if let Some(state) = self.snapshot(id) {
            self.effects.push_back(Effect::TargetChanged {
                target_id: id,
                state,
            });
        }
    }
    pub(crate) fn position(&self, id: TargetId) -> (f64, f64) {
        self.normal(id).map(|s| (s.x, s.y)).unwrap_or((0.0, 0.0))
    }
    pub(crate) fn set_position(&mut self, id: TargetId, x: f64, y: f64) {
        if let Some(state) = self.normal_mut(id) {
            if x.is_finite() {
                state.x = x;
            }
            if y.is_finite() {
                state.y = y;
            }
            self.changed(id);
        }
    }
    pub(crate) fn set_direction(&mut self, id: TargetId, direction: f64) {
        if let Some(state) = self.normal_mut(id) {
            if direction.is_finite() {
                state.direction = (direction + 180.0).rem_euclid(360.0) - 180.0;
                if state.direction == -180.0 {
                    state.direction = 180.0;
                }
            }
            self.changed(id);
        }
    }
    pub(crate) fn find_target(&self, name: &str) -> Option<TargetId> {
        let mut ids: Vec<_> = self
            .project
            .targets
            .iter()
            .filter(|(_, target)| {
                if name == "_stage_" {
                    matches!(target, Target::Stage { .. })
                } else {
                    target.sprite().scratch.name == name && !matches!(target, Target::Clone { .. })
                }
            })
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        ids.into_iter().next()
    }
    pub(crate) fn destination(&self, name: &str) -> Option<(f64, f64)> {
        if name == "_random_" {
            Some((
                rand::random_range(-240.0..=240.0),
                rand::random_range(-180.0..=180.0),
            ))
        } else {
            self.find_target(name).map(|id| self.position(id))
        }
    }
    pub(crate) fn position_action(
        &mut self,
        thread: &mut VmThread,
        action: PositionAction,
        to: (f64, f64),
    ) {
        match action {
            PositionAction::Goto => self.set_position(thread.target_id, to.0, to.1),
            PositionAction::Point => {
                let from = self.position(thread.target_id);
                if from != to {
                    self.set_direction(
                        thread.target_id,
                        (to.0 - from.0).atan2(to.1 - from.1).to_degrees(),
                    );
                }
            }
            PositionAction::Glide(secs) => {
                thread.tasks.push(Task::Glide {
                    from: self.position(thread.target_id),
                    to,
                    start: self.clock,
                    duration: duration(secs),
                });
            }
        }
    }
    pub(crate) fn volume(&self, id: TargetId) -> f64 {
        match self.project.targets.get(&id) {
            Some(Target::Stage { state, .. }) => state.volume,
            Some(Target::Normal { state, .. } | Target::Clone { state, .. }) => state.volume,
            None => 100.0,
        }
    }
    pub(crate) fn set_volume(&mut self, id: TargetId, volume: f64) {
        let volume = volume.clamp(0.0, 100.0);
        match self.project.targets.get_mut(&id) {
            Some(Target::Stage { state, .. }) => state.volume = volume,
            Some(Target::Normal { state, .. } | Target::Clone { state, .. }) => {
                state.volume = volume
            }
            None => return,
        }
        self.changed(id);
    }
    pub(crate) fn set_rotation(&mut self, id: TargetId, style: &str) {
        if let Some(state) = self.normal_mut(id) {
            state.rotation_style = match style {
                "left-right" => RotationStyle::LeftRight,
                "don't rotate" => RotationStyle::DontRotate,
                _ => RotationStyle::AllAround,
            };
            self.changed(id);
        }
    }
    pub(crate) fn reorder(&mut self, id: TargetId, index: i64) {
        let mut targets: Vec<_> = self
            .project
            .targets
            .keys()
            .copied()
            .filter(|id| self.normal(*id).is_some())
            .collect();
        targets.sort_by_key(|id| (self.normal(*id).map(|s| s.layer_order).unwrap_or(0), *id));
        targets.retain(|target| *target != id);
        targets.insert(index.clamp(0, targets.len() as i64) as usize, id);
        for (layer, target) in targets.into_iter().enumerate() {
            if let Some(state) = self.normal_mut(target) {
                state.layer_order = layer as i32;
                self.changed(target);
            }
        }
    }
}
