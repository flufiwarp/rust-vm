use std::{collections::HashMap, sync::Arc};

use serde_json::Value;

mod block;
mod error;
#[cfg(feature = "scratch")]
mod scratch;
mod sprite;
mod target;
mod utils;
#[cfg(feature = "scratch")]
pub use scratch::{ScratchAsset, ScratchList, ScratchSpriteData, ScratchVariable};

// re-exports

pub use target::{NormalTargetState, RotationStyle, StageTargetState, Target, TargetId};

pub use block::{Block, BlockId, BlockOpcode, Field, Input, InputValue, Primitive};

pub use sprite::{Sprite, SpriteId};

pub use error::ProjectError;

use crate::project::utils::json_array;

pub struct Project {
    pub meta: Value,

    pub targets: HashMap<TargetId, Target>,
    pub sprites: HashMap<SpriteId, Arc<Sprite>>,
}

impl Project {
    pub fn load_json(value: &Value) -> Result<Project, ProjectError> {
        let meta = value.get("meta").cloned().unwrap_or(Value::Null);

        let mut targets = HashMap::new();
        let mut sprites = HashMap::new();
        let targets_json = value
            .get("targets")
            .ok_or(ProjectError::MissingField("targets"))?;

        for target_json in json_array(targets_json, "targets")? {
            let target = Target::load_json(target_json)?;

            let sprite = target.sprite().clone();
            sprites.insert(sprite.uuid, sprite);

            targets.insert(TargetId::new_v4(), target);
        }

        // TODO: monitors
        // TODO: extensions
        // TODO: broadcast messages

        Ok(Project {
            meta,
            targets,
            sprites,
        })
    }
}
