use std::sync::Arc;

use serde_json::Value;
use uuid::Uuid;

use crate::project::{
    ProjectError,
    sprite::Sprite,
    utils::{bool_field, f64_field, i32_field, json_str, usize_field},
};

pub type TargetId = Uuid;

#[derive(Clone)]
pub enum Target {
    Stage {
        sprite: Arc<Sprite>,
        state: StageTargetState,
    },
    Normal {
        sprite: Arc<Sprite>,
        state: NormalTargetState,
    },
    Clone {
        sprite: Arc<Sprite>,
        state: NormalTargetState,
    },
}

impl Target {
    pub fn sprite(&self) -> &Arc<Sprite> {
        match self {
            Target::Stage { sprite, .. }
            | Target::Normal { sprite, .. }
            | Target::Clone { sprite, .. } => sprite,
        }
    }

    pub fn load_json(value: &Value) -> Result<Target, ProjectError> {
        // TODO: target name (used as the target's identity in `project.json`)
        let is_stage = bool_field(value, "isStage", false)?;
        let sprite = Arc::new(Sprite::load_json(value)?);

        if is_stage {
            Ok(Target::Stage {
                sprite,
                state: StageTargetState::load_json(value)?,
            })
        } else {
            Ok(Target::Normal {
                sprite,
                state: NormalTargetState::load_json(value)?,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StageTargetState {
    // TODO: variables
    // TODO: lists
    pub current_backdrop: usize,
    pub volume: f64,
}

impl StageTargetState {
    pub fn load_json(value: &Value) -> Result<StageTargetState, ProjectError> {
        Ok(StageTargetState {
            current_backdrop: usize_field(value, "currentCostume", 0)?,
            volume: f64_field(value, "volume", 100.0)?,
            // TODO: variables
            // TODO: lists
            // TODO: tempo
            // TODO: video state / video transparency
            // TODO: text to speech language
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalTargetState {
    pub x: f64,
    pub y: f64,
    pub direction: f64,

    pub size: f64,
    pub visible: bool,
    pub draggable: bool,

    pub rotation_style: RotationStyle,

    pub current_costume: usize,
    pub volume: f64,

    // TODO: effects

    // TODO: variables
    // TODO: lists
    pub layer_order: i32,
    pub is_clone: bool,
}

impl NormalTargetState {
    pub fn load_json(value: &Value) -> Result<NormalTargetState, ProjectError> {
        let rotation_style = match value.get("rotationStyle") {
            None | Some(Value::Null) => RotationStyle::AllAround,
            Some(value) => RotationStyle::load_json(value)?,
        };

        Ok(NormalTargetState {
            x: f64_field(value, "x", 0.0)?,
            y: f64_field(value, "y", 0.0)?,
            direction: f64_field(value, "direction", 90.0)?,

            size: f64_field(value, "size", 100.0)?,
            visible: bool_field(value, "visible", true)?,
            draggable: bool_field(value, "draggable", false)?,

            rotation_style,

            current_costume: usize_field(value, "currentCostume", 0)?,
            volume: f64_field(value, "volume", 100.0)?,

            // TODO: effects
            // TODO: variables
            // TODO: lists
            layer_order: i32_field(value, "layerOrder", 0)?,
            is_clone: false,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationStyle {
    AllAround,
    LeftRight,
    DontRotate,
}

impl RotationStyle {
    pub fn load_json(value: &Value) -> Result<RotationStyle, ProjectError> {
        match json_str(value, "rotationStyle")? {
            "all around" => Ok(RotationStyle::AllAround),
            "left-right" => Ok(RotationStyle::LeftRight),
            "don't rotate" => Ok(RotationStyle::DontRotate),
            other => Err(ProjectError::invalid("rotationStyle", other)),
        }
    }
}
