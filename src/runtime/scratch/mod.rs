use crate::{Project, ScratchList, ScratchVariable, TargetId};
use std::{collections::HashMap, time::Duration};

#[cfg(feature = "scratch-addons-debugger")]
mod addons;
mod data;
mod execute;
mod legacy;
mod motion;
mod operators;
mod procedures;
#[cfg(feature = "scratch-addons-debugger")]
pub(crate) use addons::DebuggerState;
#[cfg(feature = "scratch-addons-debugger")]
pub use addons::{DebuggerEvent, DebuggerLogLevel};
pub(crate) use procedures::argument_keys;

#[derive(Debug, Clone, Default)]
pub struct ScratchTargetState {
    pub variables: HashMap<String, ScratchVariable>,
    pub lists: HashMap<String, ScratchList>,
    pub graphic_effects: HashMap<String, f64>,
    pub sound_effects: HashMap<String, f64>,
    pub(crate) bubble_generation: u64,
}

#[derive(Debug)]
pub struct ScratchState {
    pub targets: HashMap<TargetId, ScratchTargetState>,
    pub answer: String,
    pub counter: f64,
    pub tempo: f64,
    pub voice: String,
    pub language: String,
    pub video_state: String,
    pub video_transparency: f64,
    pub(crate) timer_started: Duration,
}

impl ScratchState {
    pub(crate) fn new(project: &Project) -> Self {
        let stage = project
            .targets
            .values()
            .find(|target| matches!(target, crate::Target::Stage { .. }))
            .map(|target| &target.sprite().scratch);
        Self {
            targets: project
                .targets
                .iter()
                .map(|(id, target)| {
                    (
                        *id,
                        ScratchTargetState {
                            variables: target.sprite().scratch.variables.clone(),
                            lists: target.sprite().scratch.lists.clone(),
                            ..ScratchTargetState::default()
                        },
                    )
                })
                .collect(),
            answer: String::new(),
            counter: 0.0,
            tempo: stage.map(|s| s.tempo).unwrap_or(60.0),
            voice: "ALTO".to_string(),
            language: stage
                .map(|s| s.text_to_speech_language.clone())
                .unwrap_or_else(|| "en".to_string()),
            video_state: stage
                .map(|s| s.video_state.clone())
                .unwrap_or_else(|| "off".to_string()),
            video_transparency: stage.map(|s| s.video_transparency).unwrap_or(50.0),
            timer_started: Duration::ZERO,
        }
    }
}

pub(super) fn duration(seconds: f64) -> Duration {
    if seconds.is_nan() || seconds <= 0.0 {
        Duration::ZERO
    } else {
        Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)
    }
}
