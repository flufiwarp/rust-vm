use std::collections::HashMap;

use serde_json::Value;
use uuid::Uuid;

use crate::project::{
    ProjectError,
    block::{Block, BlockId, BlockOpcode},
    utils::json_object,
};

pub type SpriteId = Uuid;

pub struct Sprite {
    #[cfg(feature = "scratch")]
    pub scratch: super::ScratchSpriteData,
    pub uuid: SpriteId,

    pub blocks: HashMap<BlockId, Block>,

    pub top_levels: Vec<(BlockOpcode, BlockId)>,
    // TODO: clones? (counter, can just search targets)
    // TODO: sounds
    // TODO: costumes
}

impl Sprite {
    pub fn load_json(value: &Value) -> Result<Sprite, ProjectError> {
        let mut blocks = HashMap::new();
        let mut top_levels = Vec::new();

        if let Some(blocks_value) = value.get("blocks") {
            for (id, block_value) in json_object(blocks_value, "blocks")? {
                let block = Block::load_json(id.clone(), block_value)?;

                if is_top_level(block_value) {
                    top_levels.push((block.opcode.clone(), block.id.clone()));
                }

                blocks.insert(block.id.clone(), block);
            }
        }

        let uuid = SpriteId::new_v4();

        // TODO: sprite name
        // TODO: costumes
        // TODO: sounds
        // TODO: variables
        // TODO: lists
        // TODO: comments

        Ok(Sprite {
            uuid,
            blocks,
            top_levels,
            #[cfg(feature = "scratch")]
            scratch: super::ScratchSpriteData::load_json(value)?,
        })
    }

    pub fn find_blocks(&self, opcode: &BlockOpcode) -> Vec<BlockId> {
        let mut ids = Vec::new();

        for block in self.blocks.values() {
            if &block.opcode == opcode {
                ids.push(block.id.clone());
            }
        }

        ids
    }
    pub fn find_top_level(&self, opcode: &BlockOpcode) -> Vec<BlockId> {
        let mut ids = Vec::new();

        for block in self.top_levels.iter() {
            if &block.0 == opcode {
                ids.push(block.1.clone());
            }
        }

        ids
    }
}

fn is_top_level(value: &Value) -> bool {
    let top_level = value
        .get("topLevel")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    top_level
}

fn block_opcode(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) => map.get("opcode")?.as_str(),
        Value::Array(values) => values.first()?.as_str(),
        _ => None,
    }
}

// fn is_hat_opcode(opcode: &str) -> bool {
//     let mut is = false;
//     #[cfg(feature = "scratch")]
//     {
//         is = is
//             || matches!(
//                 opcode,
//                 "event_whenflagclicked"
//                     | "event_whenkeypressed"
//                     | "event_whenthisspriteclicked"
//                     | "event_whenstageclicked"
//                     | "event_whenbackdropswitchesto"
//                     | "event_whengreaterthan"
//                     | "event_whenbroadcastreceived"
//                     | "control_start_as_clone"
//                     | "procedures_definition"
//                     | "videoSensing_whenMotionGreaterThan"
//             );
//     }
//     is
// }
