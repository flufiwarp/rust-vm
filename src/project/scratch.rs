use super::{ProjectError, utils::json_object};
use crate::Value;
use serde_json::Value as Json;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ScratchVariable {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone)]
pub struct ScratchList {
    pub name: String,
    pub items: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct ScratchAsset {
    pub name: String,
    pub metadata: Json,
}

#[derive(Debug, Clone, Default)]
pub struct ScratchSpriteData {
    pub name: String,
    pub variables: HashMap<String, ScratchVariable>,
    pub lists: HashMap<String, ScratchList>,
    pub costumes: Vec<ScratchAsset>,
    pub sounds: Vec<ScratchAsset>,
    pub tempo: f64,
    pub video_state: String,
    pub video_transparency: f64,
    pub text_to_speech_language: String,
}

impl ScratchSpriteData {
    pub(crate) fn load_json(value: &Json) -> Result<Self, ProjectError> {
        let mut data = Self {
            name: value
                .get("name")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string(),
            tempo: super::utils::f64_field(value, "tempo", 60.0)?.clamp(20.0, 500.0),
            video_state: value
                .get("videoState")
                .and_then(Json::as_str)
                .unwrap_or("off")
                .to_string(),
            video_transparency: super::utils::f64_field(value, "videoTransparency", 50.0)?
                .clamp(0.0, 100.0),
            text_to_speech_language: value
                .get("textToSpeechLanguage")
                .and_then(Json::as_str)
                .unwrap_or("en")
                .to_string(),
            ..Self::default()
        };
        if let Some(variables) = value.get("variables") {
            for (id, variable) in json_object(variables, "variables")? {
                let name = variable
                    .get(0)
                    .and_then(Json::as_str)
                    .ok_or_else(|| ProjectError::invalid("variables", "missing variable name"))?;
                let value = variable
                    .get(1)
                    .and_then(Value::from_json)
                    .ok_or_else(|| ProjectError::invalid("variables", "invalid variable value"))?;
                data.variables.insert(
                    id.clone(),
                    ScratchVariable {
                        name: name.to_string(),
                        value,
                    },
                );
            }
        }
        if let Some(lists) = value.get("lists") {
            for (id, list) in json_object(lists, "lists")? {
                let name = list
                    .get(0)
                    .and_then(Json::as_str)
                    .ok_or_else(|| ProjectError::invalid("lists", "missing list name"))?;
                let items = list
                    .get(1)
                    .and_then(Json::as_array)
                    .ok_or_else(|| ProjectError::invalid("lists", "invalid list contents"))?;
                let items = items
                    .iter()
                    .map(|item| {
                        Value::from_json(item)
                            .ok_or_else(|| ProjectError::invalid("lists", "invalid list item"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                data.lists.insert(
                    id.clone(),
                    ScratchList {
                        name: name.to_string(),
                        items,
                    },
                );
            }
        }
        fn assets(value: Option<&Json>) -> Result<Vec<ScratchAsset>, ProjectError> {
            let Some(value) = value else {
                return Ok(Vec::new());
            };
            value
                .as_array()
                .ok_or_else(|| ProjectError::invalid("assets", "expected array"))?
                .iter()
                .map(|asset| {
                    let name = asset
                        .get("name")
                        .and_then(Json::as_str)
                        .ok_or_else(|| ProjectError::invalid("assets", "missing asset name"))?;
                    Ok(ScratchAsset {
                        name: name.to_string(),
                        metadata: asset.clone(),
                    })
                })
                .collect()
        }
        data.costumes = assets(value.get("costumes"))?;
        data.sounds = assets(value.get("sounds"))?;
        Ok(data)
    }
}
