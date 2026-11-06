use std::collections::HashSet;

use crate::runtime::{error::CompileError, instruction::Instruction};
use crate::{Block, BlockId, BlockOpcode, Project, Sprite, TargetId};

#[cfg(feature = "scratch")]
use crate::{Field, InputValue, Primitive, Value};
#[cfg(feature = "scratch")]
use std::collections::HashMap;

#[cfg(test)]
mod tests;

pub fn compile_block(
    project: &Project,
    block_id: &BlockId,
    target_id: &TargetId,
) -> Result<Vec<Instruction>, CompileError> {
    let target = project
        .targets
        .get(target_id)
        .ok_or(CompileError::TargetNotFound(*target_id))?;

    Compiler {
        sprite: target.sprite(),
        active: HashSet::new(),
    }
    .stack(block_id)
}

struct Compiler<'a> {
    sprite: &'a Sprite,
    // Only the current traversal path is active, so sharing a reporter is valid.
    active: HashSet<BlockId>,
}

impl Compiler<'_> {
    fn stack(&mut self, id: &BlockId) -> Result<Vec<Instruction>, CompileError> {
        let mut path = Vec::new();
        let result = (|| {
            let mut next = Some(id.clone());
            let mut instructions = Vec::new();
            while let Some(id) = next {
                if !self.active.insert(id.clone()) {
                    return Err(CompileError::CyclicBlock(id));
                }
                path.push(id.clone());
                let block = self
                    .sprite
                    .blocks
                    .get(&id)
                    .ok_or_else(|| CompileError::BlockNotFound(id.clone()))?;
                instructions.push(self.instruction(block)?);
                next = block.next.clone();
            }
            Ok(instructions)
        })();
        for id in path {
            self.active.remove(&id);
        }
        result
    }

    #[cfg(not(feature = "scratch"))]
    fn instruction(&mut self, block: &Block) -> Result<Instruction, CompileError> {
        match &block.opcode {
            BlockOpcode::Yield => Ok(Instruction::Yield),
            BlockOpcode::Other(_) => Err(CompileError::UnsupportedOpcode(block.opcode.clone())),
        }
    }
}

#[cfg(feature = "scratch")]
impl Compiler<'_> {
    fn expression(&mut self, id: &BlockId) -> Result<Instruction, CompileError> {
        if !self.active.insert(id.clone()) {
            return Err(CompileError::CyclicBlock(id.clone()));
        }
        let result = (|| {
            let block = self
                .sprite
                .blocks
                .get(id)
                .ok_or_else(|| CompileError::BlockNotFound(id.clone()))?;
            self.instruction(block)
        })();
        self.active.remove(id);
        result
    }

    fn value(&mut self, value: &InputValue) -> Result<Instruction, CompileError> {
        Ok(match value {
            InputValue::Empty => Instruction::Literal(Value::Boolean(false)),
            InputValue::Block(id) => return self.expression(id),
            InputValue::Primitive(primitive) => match primitive {
                Primitive::Number(value)
                | Primitive::PositiveNumber(value)
                | Primitive::WholeNumber(value)
                | Primitive::Integer(value)
                | Primitive::Angle(value) => Instruction::Literal(Value::Number(*value)),
                Primitive::Color(value) | Primitive::Text(value) => {
                    Instruction::Literal(Value::String(value.clone()))
                }
                Primitive::Broadcast { name, id } => Instruction::EventBroadcastMenu {
                    broadcast: Self::reference(name, id),
                },
                Primitive::Variable { name, id } => Instruction::Variable {
                    variable: Self::reference(name, id),
                },
                Primitive::List { name, id } => Instruction::ListContents {
                    list: Self::reference(name, id),
                },
            },
        })
    }

    fn reference(name: &str, id: &str) -> Field {
        Field {
            value: Value::String(name.to_string()),
            id: Some(id.to_string()),
        }
    }

    fn input(&mut self, block: &Block, name: &str) -> Result<Box<Instruction>, CompileError> {
        let input = block
            .inputs
            .get(name)
            .ok_or_else(|| CompileError::MissingInput {
                block: block.id.clone(),
                input: name.to_string(),
            })?;
        self.value(input.value()).map(Box::new)
    }

    fn condition(&mut self, block: &Block, name: &str) -> Result<Box<Instruction>, CompileError> {
        // Scratch omits unconnected boolean sockets from the input map.
        if !block.inputs.contains_key(name) {
            return Ok(Box::new(Instruction::Literal(Value::Boolean(false))));
        }
        self.input(block, name)
    }

    fn field(&self, block: &Block, name: &str) -> Result<Field, CompileError> {
        block
            .fields
            .get(name)
            .cloned()
            .ok_or_else(|| CompileError::MissingField {
                block: block.id.clone(),
                field: name.to_string(),
            })
    }

    fn substack(&mut self, block: &Block, name: &str) -> Result<Vec<Instruction>, CompileError> {
        match block.inputs.get(name).map(|input| input.value()) {
            None | Some(InputValue::Empty) => Ok(Vec::new()),
            Some(InputValue::Block(id)) => self.stack(id),
            Some(InputValue::Primitive(_)) => Err(CompileError::InvalidSubstack {
                block: block.id.clone(),
                input: name.to_string(),
            }),
        }
    }

    fn mutation(&self, block: &Block) -> Result<serde_json::Value, CompileError> {
        block
            .mutation
            .clone()
            .ok_or_else(|| CompileError::MissingMutation(block.id.clone()))
    }

    fn arguments(&mut self, block: &Block) -> Result<HashMap<String, Instruction>, CompileError> {
        block
            .inputs
            .iter()
            .map(|(id, input)| Ok((id.clone(), self.value(input.value())?)))
            .collect()
    }

    fn instruction(&mut self, block: &Block) -> Result<Instruction, CompileError> {
        macro_rules! input {
            ($name:literal) => {
                self.input(block, $name)?
            };
        }
        macro_rules! condition {
            ($name:literal) => {
                self.condition(block, $name)?
            };
        }
        macro_rules! field {
            ($name:literal) => {
                self.field(block, $name)?
            };
        }
        macro_rules! body {
            ($name:literal) => {
                self.substack(block, $name)?
            };
        }

        Ok(match &block.opcode {
            // Motion
            BlockOpcode::MoveSteps => Instruction::MoveSteps {
                steps: input!("STEPS"),
            },
            BlockOpcode::TurnRight => Instruction::TurnRight {
                degrees: input!("DEGREES"),
            },
            BlockOpcode::TurnLeft => Instruction::TurnLeft {
                degrees: input!("DEGREES"),
            },
            BlockOpcode::Goto => Instruction::Goto { to: input!("TO") },
            BlockOpcode::GotoXY => Instruction::GotoXY {
                x: input!("X"),
                y: input!("Y"),
            },
            BlockOpcode::GlideTo => Instruction::GlideTo {
                secs: input!("SECS"),
                to: input!("TO"),
            },
            BlockOpcode::GlideSecsToXY => Instruction::GlideSecsToXY {
                secs: input!("SECS"),
                x: input!("X"),
                y: input!("Y"),
            },
            BlockOpcode::PointInDirection => Instruction::PointInDirection {
                direction: input!("DIRECTION"),
            },
            BlockOpcode::PointTowards => Instruction::PointTowards {
                towards: input!("TOWARDS"),
            },
            BlockOpcode::ChangeXBy => Instruction::ChangeXBy { dx: input!("DX") },
            BlockOpcode::SetX => Instruction::SetX { x: input!("X") },
            BlockOpcode::ChangeYBy => Instruction::ChangeYBy { dy: input!("DY") },
            BlockOpcode::SetY => Instruction::SetY { y: input!("Y") },
            BlockOpcode::IfOnEdgeBounce => Instruction::IfOnEdgeBounce,
            BlockOpcode::SetRotationStyle => Instruction::SetRotationStyle {
                style: field!("STYLE"),
            },
            BlockOpcode::XPosition => Instruction::XPosition,
            BlockOpcode::YPosition => Instruction::YPosition,
            BlockOpcode::Direction => Instruction::Direction,
            BlockOpcode::ScrollRight => Instruction::ScrollRight {
                distance: input!("DISTANCE"),
            },
            BlockOpcode::ScrollUp => Instruction::ScrollUp {
                distance: input!("DISTANCE"),
            },
            BlockOpcode::AlignScene => Instruction::AlignScene {
                alignment: field!("ALIGNMENT"),
            },
            BlockOpcode::XScroll => Instruction::XScroll,
            BlockOpcode::YScroll => Instruction::YScroll,

            // Looks
            BlockOpcode::SayForSecs => Instruction::SayForSecs {
                message: input!("MESSAGE"),
                secs: input!("SECS"),
            },
            BlockOpcode::Say => Instruction::Say {
                message: input!("MESSAGE"),
            },
            BlockOpcode::ThinkForSecs => Instruction::ThinkForSecs {
                message: input!("MESSAGE"),
                secs: input!("SECS"),
            },
            BlockOpcode::Think => Instruction::Think {
                message: input!("MESSAGE"),
            },
            BlockOpcode::SwitchCostumeTo => Instruction::SwitchCostumeTo {
                costume: input!("COSTUME"),
            },
            BlockOpcode::NextCostume => Instruction::NextCostume,
            BlockOpcode::SwitchBackdropTo => Instruction::SwitchBackdropTo {
                backdrop: input!("BACKDROP"),
            },
            BlockOpcode::SwitchBackdropToAndWait => Instruction::SwitchBackdropToAndWait {
                backdrop: input!("BACKDROP"),
            },
            BlockOpcode::NextBackdrop => Instruction::NextBackdrop,
            BlockOpcode::ChangeSizeBy => Instruction::ChangeSizeBy {
                change: input!("CHANGE"),
            },
            BlockOpcode::SetSizeTo => Instruction::SetSizeTo {
                size: input!("SIZE"),
            },
            BlockOpcode::ChangeEffectBy => Instruction::ChangeEffectBy {
                effect: field!("EFFECT"),
                change: input!("CHANGE"),
            },
            BlockOpcode::SetEffectTo => Instruction::SetEffectTo {
                effect: field!("EFFECT"),
                value: input!("VALUE"),
            },
            BlockOpcode::ClearGraphicEffects => Instruction::ClearGraphicEffects,
            BlockOpcode::Show => Instruction::Show,
            BlockOpcode::Hide => Instruction::Hide,
            BlockOpcode::GoToFrontBack => Instruction::GoToFrontBack {
                front_back: field!("FRONT_BACK"),
            },
            BlockOpcode::GoForwardBackwardLayers => Instruction::GoForwardBackwardLayers {
                forward_backward: field!("FORWARD_BACKWARD"),
                layers: input!("NUM"),
            },
            BlockOpcode::CostumeNumberName => Instruction::CostumeNumberName {
                number_name: field!("NUMBER_NAME"),
            },
            BlockOpcode::BackdropNumberName => Instruction::BackdropNumberName {
                number_name: field!("NUMBER_NAME"),
            },
            BlockOpcode::Size => Instruction::Size,
            BlockOpcode::HideAllSprites => Instruction::HideAllSprites,
            BlockOpcode::ChangeStretchBy => Instruction::ChangeStretchBy {
                change: input!("CHANGE"),
            },
            BlockOpcode::SetStretchTo => Instruction::SetStretchTo {
                stretch: input!("STRETCH"),
            },

            // Sound
            BlockOpcode::PlaySoundUntilDone => Instruction::PlaySoundUntilDone {
                sound: input!("SOUND_MENU"),
            },
            BlockOpcode::PlaySound => Instruction::PlaySound {
                sound: input!("SOUND_MENU"),
            },
            BlockOpcode::StopAllSounds => Instruction::StopAllSounds,
            BlockOpcode::ChangeSoundEffectBy => Instruction::ChangeSoundEffectBy {
                effect: field!("EFFECT"),
                value: input!("VALUE"),
            },
            BlockOpcode::SetSoundEffectTo => Instruction::SetSoundEffectTo {
                effect: field!("EFFECT"),
                value: input!("VALUE"),
            },
            BlockOpcode::ClearSoundEffects => Instruction::ClearSoundEffects,
            BlockOpcode::ChangeVolumeBy => Instruction::ChangeVolumeBy {
                volume: input!("VOLUME"),
            },
            BlockOpcode::SetVolumeTo => Instruction::SetVolumeTo {
                volume: input!("VOLUME"),
            },
            BlockOpcode::Volume => Instruction::Volume,
            BlockOpcode::SoundBeatsMenu => Instruction::SoundBeatsMenu {
                beats: field!("BEATS"),
            },
            BlockOpcode::SoundEffectsMenu => Instruction::SoundEffectsMenu {
                effect: field!("EFFECT"),
            },

            // Events
            BlockOpcode::WhenFlagClicked => Instruction::WhenFlagClicked,
            BlockOpcode::WhenKeyPressed => Instruction::WhenKeyPressed {
                key: field!("KEY_OPTION"),
            },
            BlockOpcode::WhenThisSpriteClicked => Instruction::WhenThisSpriteClicked,
            BlockOpcode::WhenStageClicked => Instruction::WhenStageClicked,
            BlockOpcode::WhenBackdropSwitchesTo => Instruction::WhenBackdropSwitchesTo {
                backdrop: field!("BACKDROP"),
            },
            BlockOpcode::WhenGreaterThan => Instruction::WhenGreaterThan {
                sensor: field!("WHENGREATERTHANMENU"),
                value: input!("VALUE"),
            },
            BlockOpcode::WhenIReceive => Instruction::WhenIReceive {
                broadcast: field!("BROADCAST_OPTION"),
            },
            BlockOpcode::WhenTouchingObject => Instruction::WhenTouchingObject {
                object: input!("TOUCHINGOBJECTMENU"),
            },
            BlockOpcode::Broadcast => Instruction::Broadcast {
                broadcast: input!("BROADCAST_INPUT"),
            },
            BlockOpcode::BroadcastAndWait => Instruction::BroadcastAndWait {
                broadcast: input!("BROADCAST_INPUT"),
            },

            // Control
            BlockOpcode::Wait => Instruction::Wait {
                duration: input!("DURATION"),
            },
            BlockOpcode::Repeat => Instruction::Repeat {
                times: input!("TIMES"),
                body: body!("SUBSTACK"),
            },
            BlockOpcode::Forever => Instruction::Forever {
                body: body!("SUBSTACK"),
            },
            BlockOpcode::If => Instruction::If {
                condition: condition!("CONDITION"),
                body: body!("SUBSTACK"),
            },
            BlockOpcode::IfElse => Instruction::IfElse {
                condition: condition!("CONDITION"),
                body: body!("SUBSTACK"),
                else_body: body!("SUBSTACK2"),
            },
            BlockOpcode::WaitUntil => Instruction::WaitUntil {
                condition: condition!("CONDITION"),
            },
            BlockOpcode::RepeatUntil => Instruction::RepeatUntil {
                condition: condition!("CONDITION"),
                body: body!("SUBSTACK"),
            },
            BlockOpcode::While => Instruction::While {
                condition: condition!("CONDITION"),
                body: body!("SUBSTACK"),
            },
            BlockOpcode::ForEach => Instruction::ForEach {
                variable: field!("VARIABLE"),
                value: input!("VALUE"),
                body: body!("SUBSTACK"),
            },
            BlockOpcode::Stop => Instruction::Stop {
                option: field!("STOP_OPTION"),
            },
            BlockOpcode::StartAsClone => Instruction::StartAsClone,
            BlockOpcode::CreateCloneOf => Instruction::CreateCloneOf {
                clone: input!("CLONE_OPTION"),
            },
            BlockOpcode::DeleteThisClone => Instruction::DeleteThisClone,
            BlockOpcode::GetCounter => Instruction::GetCounter,
            BlockOpcode::IncrCounter => Instruction::IncrCounter,
            BlockOpcode::ClearCounter => Instruction::ClearCounter,
            BlockOpcode::AllAtOnce => Instruction::AllAtOnce {
                body: body!("SUBSTACK"),
            },

            // Sensing
            BlockOpcode::TouchingObject => Instruction::TouchingObject {
                object: input!("TOUCHINGOBJECTMENU"),
            },
            BlockOpcode::TouchingColor => Instruction::TouchingColor {
                color: input!("COLOR"),
            },
            BlockOpcode::ColorIsTouchingColor => Instruction::ColorIsTouchingColor {
                color: input!("COLOR"),
                color2: input!("COLOR2"),
            },
            BlockOpcode::DistanceTo => Instruction::DistanceTo {
                to: input!("DISTANCETOMENU"),
            },
            BlockOpcode::AskAndWait => Instruction::AskAndWait {
                question: input!("QUESTION"),
            },
            BlockOpcode::Answer => Instruction::Answer,
            BlockOpcode::KeyPressed => Instruction::KeyPressed {
                key: input!("KEY_OPTION"),
            },
            BlockOpcode::MouseDown => Instruction::MouseDown,
            BlockOpcode::MouseX => Instruction::MouseX,
            BlockOpcode::MouseY => Instruction::MouseY,
            BlockOpcode::SetDragMode => Instruction::SetDragMode {
                mode: field!("DRAG_MODE"),
            },
            BlockOpcode::Loudness => Instruction::Loudness,
            BlockOpcode::Loud => Instruction::Loud,
            BlockOpcode::Timer => Instruction::Timer,
            BlockOpcode::ResetTimer => Instruction::ResetTimer,
            BlockOpcode::Of => Instruction::Of {
                property: field!("PROPERTY"),
                object: input!("OBJECT"),
            },
            BlockOpcode::Current => Instruction::Current {
                current_menu: field!("CURRENTMENU"),
            },
            BlockOpcode::DaysSince2000 => Instruction::DaysSince2000,
            BlockOpcode::Username => Instruction::Username,
            BlockOpcode::UserId => Instruction::UserId,

            // Operators
            BlockOpcode::Add => Instruction::Add {
                num1: input!("NUM1"),
                num2: input!("NUM2"),
            },
            BlockOpcode::Subtract => Instruction::Subtract {
                num1: input!("NUM1"),
                num2: input!("NUM2"),
            },
            BlockOpcode::Multiply => Instruction::Multiply {
                num1: input!("NUM1"),
                num2: input!("NUM2"),
            },
            BlockOpcode::Divide => Instruction::Divide {
                num1: input!("NUM1"),
                num2: input!("NUM2"),
            },
            BlockOpcode::Random => Instruction::Random {
                from: input!("FROM"),
                to: input!("TO"),
            },
            BlockOpcode::GreaterThan => Instruction::GreaterThan {
                operand1: input!("OPERAND1"),
                operand2: input!("OPERAND2"),
            },
            BlockOpcode::LessThan => Instruction::LessThan {
                operand1: input!("OPERAND1"),
                operand2: input!("OPERAND2"),
            },
            BlockOpcode::Equals => Instruction::Equals {
                operand1: input!("OPERAND1"),
                operand2: input!("OPERAND2"),
            },
            BlockOpcode::And => Instruction::And {
                operand1: condition!("OPERAND1"),
                operand2: condition!("OPERAND2"),
            },
            BlockOpcode::Or => Instruction::Or {
                operand1: condition!("OPERAND1"),
                operand2: condition!("OPERAND2"),
            },
            BlockOpcode::Not => Instruction::Not {
                operand: condition!("OPERAND"),
            },
            BlockOpcode::Join => Instruction::Join {
                string1: input!("STRING1"),
                string2: input!("STRING2"),
            },
            BlockOpcode::LetterOf => Instruction::LetterOf {
                letter: input!("LETTER"),
                string: input!("STRING"),
            },
            BlockOpcode::Length => Instruction::Length {
                string: input!("STRING"),
            },
            BlockOpcode::Contains => Instruction::Contains {
                string1: input!("STRING1"),
                string2: input!("STRING2"),
            },
            BlockOpcode::Mod => Instruction::Mod {
                num1: input!("NUM1"),
                num2: input!("NUM2"),
            },
            BlockOpcode::Round => Instruction::Round { num: input!("NUM") },
            BlockOpcode::MathOp => Instruction::MathOp {
                operator: field!("OPERATOR"),
                num: input!("NUM"),
            },

            // Variables
            BlockOpcode::Variable => Instruction::Variable {
                variable: field!("VARIABLE"),
            },
            BlockOpcode::SetVariableTo => Instruction::SetVariableTo {
                variable: field!("VARIABLE"),
                value: input!("VALUE"),
            },
            BlockOpcode::ChangeVariableBy => Instruction::ChangeVariableBy {
                variable: field!("VARIABLE"),
                value: input!("VALUE"),
            },
            BlockOpcode::ShowVariable => Instruction::ShowVariable {
                variable: field!("VARIABLE"),
            },
            BlockOpcode::HideVariable => Instruction::HideVariable {
                variable: field!("VARIABLE"),
            },

            // Lists
            BlockOpcode::ListContents => Instruction::ListContents {
                list: field!("LIST"),
            },
            BlockOpcode::AddToList => Instruction::AddToList {
                list: field!("LIST"),
                item: input!("ITEM"),
            },
            BlockOpcode::DeleteOfList => Instruction::DeleteOfList {
                list: field!("LIST"),
                index: input!("INDEX"),
            },
            BlockOpcode::DeleteAllOfList => Instruction::DeleteAllOfList {
                list: field!("LIST"),
            },
            BlockOpcode::InsertAtList => Instruction::InsertAtList {
                list: field!("LIST"),
                index: input!("INDEX"),
                item: input!("ITEM"),
            },
            BlockOpcode::ReplaceItemOfList => Instruction::ReplaceItemOfList {
                list: field!("LIST"),
                index: input!("INDEX"),
                item: input!("ITEM"),
            },
            BlockOpcode::ItemOfList => Instruction::ItemOfList {
                list: field!("LIST"),
                index: input!("INDEX"),
            },
            BlockOpcode::ItemNumOfList => Instruction::ItemNumOfList {
                list: field!("LIST"),
                item: input!("ITEM"),
            },
            BlockOpcode::LengthOfList => Instruction::LengthOfList {
                list: field!("LIST"),
            },
            BlockOpcode::ListContainsItem => Instruction::ListContainsItem {
                list: field!("LIST"),
                item: input!("ITEM"),
            },
            BlockOpcode::ShowList => Instruction::ShowList {
                list: field!("LIST"),
            },
            BlockOpcode::HideList => Instruction::HideList {
                list: field!("LIST"),
            },

            // Custom blocks
            BlockOpcode::ProceduresDefinition => Instruction::ProceduresDefinition {
                prototype: input!("custom_block"),
            },
            BlockOpcode::ProceduresPrototype => Instruction::ProceduresPrototype {
                mutation: self.mutation(block)?,
                arguments: self.arguments(block)?,
            },
            BlockOpcode::ProceduresCall => Instruction::ProceduresCall {
                mutation: self.mutation(block)?,
                arguments: self.arguments(block)?,
            },
            BlockOpcode::ArgumentReporterStringNumber => {
                Instruction::ArgumentReporterStringNumber {
                    name: field!("VALUE"),
                }
            }
            BlockOpcode::ArgumentReporterBoolean => Instruction::ArgumentReporterBoolean {
                name: field!("VALUE"),
            },

            // Pen extension
            BlockOpcode::PenClear => Instruction::PenClear,
            BlockOpcode::PenStamp => Instruction::PenStamp,
            BlockOpcode::PenDown => Instruction::PenDown,
            BlockOpcode::PenUp => Instruction::PenUp,
            BlockOpcode::PenSetColor => Instruction::PenSetColor {
                color: input!("COLOR"),
            },
            BlockOpcode::PenChangeColorParamBy => Instruction::PenChangeColorParamBy {
                color_param: input!("COLOR_PARAM"),
                value: input!("VALUE"),
            },
            BlockOpcode::PenSetColorParamTo => Instruction::PenSetColorParamTo {
                color_param: input!("COLOR_PARAM"),
                value: input!("VALUE"),
            },
            BlockOpcode::PenChangeSizeBy => Instruction::PenChangeSizeBy {
                size: input!("SIZE"),
            },
            BlockOpcode::PenSetSizeTo => Instruction::PenSetSizeTo {
                size: input!("SIZE"),
            },
            BlockOpcode::PenSetShade => Instruction::PenSetShade {
                shade: input!("SHADE"),
            },
            BlockOpcode::PenChangeShadeBy => Instruction::PenChangeShadeBy {
                shade: input!("SHADE"),
            },
            BlockOpcode::PenSetHue => Instruction::PenSetHue { hue: input!("HUE") },
            BlockOpcode::PenChangeHueBy => Instruction::PenChangeHueBy { hue: input!("HUE") },

            // Music extension
            BlockOpcode::MusicPlayDrumForBeats => Instruction::MusicPlayDrumForBeats {
                drum: input!("DRUM"),
                beats: input!("BEATS"),
            },
            BlockOpcode::MusicRestForBeats => Instruction::MusicRestForBeats {
                beats: input!("BEATS"),
            },
            BlockOpcode::MusicPlayNoteForBeats => Instruction::MusicPlayNoteForBeats {
                note: input!("NOTE"),
                beats: input!("BEATS"),
            },
            BlockOpcode::MusicSetInstrument => Instruction::MusicSetInstrument {
                instrument: input!("INSTRUMENT"),
            },
            BlockOpcode::MusicSetTempo => Instruction::MusicSetTempo {
                tempo: input!("TEMPO"),
            },
            BlockOpcode::MusicChangeTempo => Instruction::MusicChangeTempo {
                tempo: input!("TEMPO"),
            },
            BlockOpcode::MusicGetTempo => Instruction::MusicGetTempo,
            BlockOpcode::MusicMidiPlayDrumForBeats => Instruction::MusicMidiPlayDrumForBeats {
                drum: input!("DRUM"),
                beats: input!("BEATS"),
            },
            BlockOpcode::MusicMidiSetInstrument => Instruction::MusicMidiSetInstrument {
                instrument: input!("INSTRUMENT"),
            },

            // Video Sensing extension
            BlockOpcode::VideoSensingWhenMotionGreaterThan => {
                Instruction::VideoSensingWhenMotionGreaterThan {
                    reference: input!("REFERENCE"),
                }
            }
            BlockOpcode::VideoSensingVideoOn => Instruction::VideoSensingVideoOn {
                attribute: input!("ATTRIBUTE"),
                subject: input!("SUBJECT"),
            },
            BlockOpcode::VideoSensingVideoToggle => Instruction::VideoSensingVideoToggle {
                video_state: input!("VIDEO_STATE"),
            },
            BlockOpcode::VideoSensingSetVideoTransparency => {
                Instruction::VideoSensingSetVideoTransparency {
                    transparency: input!("TRANSPARENCY"),
                }
            }

            // Text to Speech extension
            BlockOpcode::Text2SpeechSpeakAndWait => Instruction::Text2SpeechSpeakAndWait {
                words: input!("WORDS"),
            },
            BlockOpcode::Text2SpeechSetVoice => Instruction::Text2SpeechSetVoice {
                voice: input!("VOICE"),
            },
            BlockOpcode::Text2SpeechSetLanguage => Instruction::Text2SpeechSetLanguage {
                language: input!("LANGUAGE"),
            },

            // Translate extension
            BlockOpcode::TranslateGetTranslate => Instruction::TranslateGetTranslate {
                words: input!("WORDS"),
                language: input!("LANGUAGE"),
            },
            BlockOpcode::TranslateGetViewerLanguage => Instruction::TranslateGetViewerLanguage,

            // Menu / shadow blocks
            BlockOpcode::MotionGotoMenu => Instruction::MotionGotoMenu { to: field!("TO") },
            BlockOpcode::MotionGlideToMenu => Instruction::MotionGlideToMenu { to: field!("TO") },
            BlockOpcode::MotionPointTowardsMenu => Instruction::MotionPointTowardsMenu {
                towards: field!("TOWARDS"),
            },
            BlockOpcode::LooksCostume => Instruction::LooksCostume {
                costume: field!("COSTUME"),
            },
            BlockOpcode::LooksBackdrops => Instruction::LooksBackdrops {
                backdrop: field!("BACKDROP"),
            },
            BlockOpcode::SoundSoundsMenu => Instruction::SoundSoundsMenu {
                sound: field!("SOUND_MENU"),
            },
            BlockOpcode::EventBroadcastMenu => Instruction::EventBroadcastMenu {
                broadcast: field!("BROADCAST_OPTION"),
            },
            BlockOpcode::ControlCreateCloneOfMenu => Instruction::ControlCreateCloneOfMenu {
                clone: field!("CLONE_OPTION"),
            },
            BlockOpcode::SensingTouchingObjectMenu => Instruction::SensingTouchingObjectMenu {
                object: field!("TOUCHINGOBJECTMENU"),
            },
            BlockOpcode::SensingDistanceToMenu => Instruction::SensingDistanceToMenu {
                to: field!("DISTANCETOMENU"),
            },
            BlockOpcode::SensingKeyOptions => Instruction::SensingKeyOptions {
                key: field!("KEY_OPTION"),
            },
            BlockOpcode::SensingOfObjectMenu => Instruction::SensingOfObjectMenu {
                object: field!("OBJECT"),
            },
            BlockOpcode::PenMenuColorParam => Instruction::PenMenuColorParam {
                color_param: self
                    .field(block, "colorParam")
                    .or_else(|_| self.field(block, "COLOR_PARAM"))?,
            },
            BlockOpcode::MusicMenuDrum => Instruction::MusicMenuDrum {
                drum: field!("DRUM"),
            },
            BlockOpcode::MusicMenuInstrument => Instruction::MusicMenuInstrument {
                instrument: field!("INSTRUMENT"),
            },
            BlockOpcode::MusicNote => Instruction::MusicNote {
                note: field!("NOTE"),
            },
            BlockOpcode::VideoSensingMenuAttribute => Instruction::VideoSensingMenuAttribute {
                attribute: field!("ATTRIBUTE"),
            },
            BlockOpcode::VideoSensingMenuSubject => Instruction::VideoSensingMenuSubject {
                subject: field!("SUBJECT"),
            },
            BlockOpcode::VideoSensingMenuVideoState => Instruction::VideoSensingMenuVideoState {
                video_state: field!("VIDEO_STATE"),
            },
            BlockOpcode::Text2SpeechMenuVoices => Instruction::Text2SpeechMenuVoices {
                voice: field!("voices"),
            },
            BlockOpcode::Text2SpeechMenuLanguages => Instruction::Text2SpeechMenuLanguages {
                language: field!("languages"),
            },
            BlockOpcode::TranslateMenuLanguages => Instruction::TranslateMenuLanguages {
                language: field!("languages"),
            },

            BlockOpcode::Yield => Instruction::Yield,
            BlockOpcode::Other(_) => {
                return Err(CompileError::UnsupportedOpcode(block.opcode.clone()));
            }
        })
    }
}
