use std::collections::HashMap;

use serde_json::Value;

use crate::cfg_variants;
use crate::project::ProjectError;
use crate::project::utils::{json_is_null, json_str};
use crate::value::Value as RuntimeValue;

pub type BlockId = String;

#[derive(Debug, Clone)]
pub struct Block {
    pub id: BlockId,
    pub opcode: BlockOpcode,

    pub next: Option<BlockId>,
    pub parent: Option<BlockId>,

    pub inputs: HashMap<String, Input>,
    pub fields: HashMap<String, Field>,
    /// Custom-block signatures and other opcode-specific metadata.
    pub mutation: Option<Value>,
    // TODO: shadow
    // TODO: top_level
}

impl Block {
    pub fn load_json(id: BlockId, value: &Value) -> Result<Block, ProjectError> {
        // Detached variable and list reporters use Scratch's compact primitive
        // format in the blocks map, rather than a regular block object.
        if let Value::Array(values) = value
            && values.first().is_some_and(Value::is_number)
        {
            let (opcode, field, name, reference_id) = match Primitive::load_json("block", value)? {
                Primitive::Variable { name, id } => ("data_variable", "VARIABLE", name, id),
                Primitive::List { name, id } => ("data_listcontents", "LIST", name, id),
                _ => {
                    return Err(ProjectError::invalid(
                        "block",
                        "expected a variable or list reporter",
                    ));
                }
            };
            return Ok(Block {
                id,
                opcode: BlockOpcode::from_str(opcode),
                next: None,
                parent: None,
                inputs: HashMap::new(),
                fields: HashMap::from([(
                    field.to_string(),
                    Field {
                        value: RuntimeValue::String(name),
                        id: Some(reference_id),
                    },
                )]),
                mutation: None,
            });
        }

        let opcode = match value {
            Value::Object(map) => {
                let opcode = map
                    .get("opcode")
                    .ok_or(ProjectError::MissingField("opcode"))?;
                json_str(opcode, "opcode")?
            }
            Value::Array(values) => {
                let opcode = values.first().ok_or(ProjectError::MissingField("opcode"))?;
                json_str(opcode, "opcode")?
            }
            _ => return Err(ProjectError::unexpected("block", "object or array", value)),
        };

        let next = if let Value::Object(map) = value {
            let next = map.get("next");

            if let Some(next) = next
                && !json_is_null(next)
            {
                Some(json_str(next, "next")?.to_string())
            } else {
                None
            }
        } else {
            None
        };

        let parent = if let Value::Object(map) = value {
            let parent = map.get("parent");

            if let Some(parent) = parent
                && !json_is_null(parent)
            {
                Some(json_str(parent, "parent")?.to_string())
            } else {
                None
            }
        } else {
            None
        };

        let inputs = parse_map_field(value, "inputs", Input::load_json)?;
        let fields = parse_map_field(value, "fields", Field::load_json)?;

        // TODO: parse `shadow` / `topLevel`

        Ok(Block {
            id,
            opcode: BlockOpcode::from_str(opcode),

            next,
            parent,

            inputs,
            fields,
            mutation: value
                .get("mutation")
                .filter(|value| !value.is_null())
                .cloned(),
        })
    }
}

fn parse_map_field<T>(
    value: &Value,
    field: &'static str,
    mut parse: impl FnMut(&str, &Value) -> Result<T, ProjectError>,
) -> Result<HashMap<String, T>, ProjectError> {
    let map = match value.get(field) {
        None | Some(Value::Null) => return Ok(HashMap::new()),
        Some(map) => map
            .as_object()
            .ok_or_else(|| ProjectError::unexpected(field, "object", map))?,
    };

    let mut parsed = HashMap::with_capacity(map.len());
    for (name, item) in map {
        parsed.insert(name.clone(), parse(name, item)?);
    }

    Ok(parsed)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// `[1, value]` - the input contains only a shadow value.
    Shadow(InputValue),
    /// `[2, value]` - the input contains a block with no shadow.
    NoShadow(InputValue),
    /// `[3, value, shadow]` - a block obscures a shadow value.
    Obscured {
        value: InputValue,
        shadow: InputValue,
    },
}

impl Input {
    pub fn value(&self) -> &InputValue {
        match self {
            Input::Shadow(value) | Input::NoShadow(value) => value,
            Input::Obscured { value, .. } => value,
        }
    }

    pub fn shadow(&self) -> Option<&InputValue> {
        match self {
            Input::Shadow(value) => Some(value),
            Input::NoShadow(_) => None,
            Input::Obscured { shadow, .. } => Some(shadow),
        }
    }

    fn load_json(name: &str, value: &Value) -> Result<Input, ProjectError> {
        let descriptor = value
            .as_array()
            .ok_or_else(|| ProjectError::unexpected(name, "array", value))?;

        let kind = descriptor
            .first()
            .ok_or_else(|| ProjectError::invalid(name, "missing input type"))?;
        let kind = kind
            .as_u64()
            .ok_or_else(|| ProjectError::unexpected(format!("{name}[0]"), "integer", kind))?;

        match kind {
            1 => Ok(Input::Shadow(InputValue::load_json(
                name,
                input_element(descriptor, 1, name)?,
            )?)),
            2 => Ok(Input::NoShadow(InputValue::load_json(
                name,
                input_element(descriptor, 1, name)?,
            )?)),
            3 => Ok(Input::Obscured {
                value: InputValue::load_json(name, input_element(descriptor, 1, name)?)?,
                shadow: InputValue::load_json(name, input_element(descriptor, 2, name)?)?,
            }),
            other => Err(ProjectError::invalid(
                name,
                format!("unknown input type {other}"),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputValue {
    /// An unconnected boolean input or an empty substack.
    Empty,
    Block(BlockId),
    Primitive(Primitive),
}

impl InputValue {
    fn load_json(name: &str, value: &Value) -> Result<InputValue, ProjectError> {
        match value {
            Value::Null => Ok(InputValue::Empty),
            Value::String(id) => Ok(InputValue::Block(id.clone())),
            Value::Array(_) => Ok(InputValue::Primitive(Primitive::load_json(name, value)?)),
            _ => Err(ProjectError::unexpected(
                name,
                "block id, primitive or null",
                value,
            )),
        }
    }

    pub fn block(self) -> Result<BlockId, ProjectError> {
        if let InputValue::Block(id) = self {
            Ok(id)
        } else {
            Err(ProjectError::ExpectedBlockForInput)
        }
    }
    pub fn primitive(self) -> Result<BlockId, ProjectError> {
        if let InputValue::Block(id) = self {
            Ok(id)
        } else {
            Err(ProjectError::ExpectedPrimitiveForInput)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    Number(f64),
    PositiveNumber(f64),
    WholeNumber(f64),
    Integer(f64),
    Angle(f64),
    Color(String),
    Text(String),
    Broadcast { name: String, id: String },
    Variable { name: String, id: String },
    List { name: String, id: String },
}

impl Primitive {
    fn load_json(name: &str, value: &Value) -> Result<Primitive, ProjectError> {
        let descriptor = value
            .as_array()
            .ok_or_else(|| ProjectError::unexpected(name, "array", value))?;

        let kind = descriptor
            .first()
            .ok_or_else(|| ProjectError::invalid(name, "missing primitive type"))?;
        let kind = kind
            .as_u64()
            .ok_or_else(|| ProjectError::unexpected(format!("{name}[0]"), "integer", kind))?;

        let primitive = match kind {
            4 => Primitive::Number(primitive_number(name, input_element(descriptor, 1, name)?)?),
            5 => Primitive::PositiveNumber(primitive_number(
                name,
                input_element(descriptor, 1, name)?,
            )?),
            6 => {
                Primitive::WholeNumber(primitive_number(name, input_element(descriptor, 1, name)?)?)
            }
            7 => Primitive::Integer(primitive_number(name, input_element(descriptor, 1, name)?)?),
            8 => Primitive::Angle(primitive_number(name, input_element(descriptor, 1, name)?)?),
            9 => Primitive::Color(primitive_string(name, input_element(descriptor, 1, name)?)?),
            10 => Primitive::Text(primitive_string(name, input_element(descriptor, 1, name)?)?),
            11 => Primitive::Broadcast {
                name: primitive_string(name, input_element(descriptor, 1, name)?)?,
                id: primitive_string(name, input_element(descriptor, 2, name)?)?,
            },
            12 => Primitive::Variable {
                name: primitive_string(name, input_element(descriptor, 1, name)?)?,
                id: primitive_string(name, input_element(descriptor, 2, name)?)?,
            },
            13 => Primitive::List {
                name: primitive_string(name, input_element(descriptor, 1, name)?)?,
                id: primitive_string(name, input_element(descriptor, 2, name)?)?,
            },
            other => {
                return Err(ProjectError::invalid(
                    name,
                    format!("unknown primitive type {other}"),
                ));
            }
        };

        Ok(primitive)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub value: RuntimeValue,
    pub id: Option<String>,
}

impl Field {
    fn load_json(name: &str, value: &Value) -> Result<Field, ProjectError> {
        let descriptor = value
            .as_array()
            .ok_or_else(|| ProjectError::unexpected(name, "array", value))?;

        let value = field_value(name, input_element(descriptor, 0, name)?)?;
        let id = match descriptor.get(1) {
            None | Some(Value::Null) => None,
            Some(id) => Some(primitive_string(name, id)?),
        };

        Ok(Field { value, id })
    }
}

fn input_element<'a>(
    descriptor: &'a [Value],
    index: usize,
    field: &str,
) -> Result<&'a Value, ProjectError> {
    descriptor
        .get(index)
        .ok_or_else(|| ProjectError::invalid(field, format!("missing element at index {index}")))
}

fn primitive_number(field: &str, value: &Value) -> Result<f64, ProjectError> {
    match value {
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| ProjectError::invalid(field, number.to_string())),
        Value::String(string) => string
            .parse()
            .map_err(|_| ProjectError::invalid(field, string.clone())),
        _ => Err(ProjectError::unexpected(field, "number", value)),
    }
}

fn primitive_string(field: &str, value: &Value) -> Result<String, ProjectError> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ProjectError::unexpected(field, "string", value))
}

fn field_value(field: &str, value: &Value) -> Result<RuntimeValue, ProjectError> {
    match value {
        Value::Number(number) => number
            .as_f64()
            .map(RuntimeValue::Number)
            .ok_or_else(|| ProjectError::invalid(field, number.to_string())),
        Value::String(string) => Ok(RuntimeValue::String(string.clone())),
        Value::Bool(boolean) => Ok(RuntimeValue::Boolean(*boolean)),
        _ => Err(ProjectError::unexpected(
            field,
            "number, string or boolean",
            value,
        )),
    }
}

cfg_variants! {
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub enum BlockOpcode {
        #[cfg(feature = "scratch")] {
            // Motion
            MoveSteps,
            TurnRight,
            TurnLeft,
            Goto,
            GotoXY,
            GlideTo,
            GlideSecsToXY,
            PointInDirection,
            PointTowards,
            ChangeXBy,
            SetX,
            ChangeYBy,
            SetY,
            IfOnEdgeBounce,
            SetRotationStyle,
            XPosition,
            YPosition,
            Direction,
            // Legacy Scratch motion blocks (no-ops in Scratch 3).
            ScrollRight,
            ScrollUp,
            AlignScene,
            XScroll,
            YScroll,

            // Looks
            SayForSecs,
            Say,
            ThinkForSecs,
            Think,
            SwitchCostumeTo,
            NextCostume,
            SwitchBackdropTo,
            SwitchBackdropToAndWait,
            NextBackdrop,
            ChangeSizeBy,
            SetSizeTo,
            ChangeEffectBy,
            SetEffectTo,
            ClearGraphicEffects,
            Show,
            Hide,
            GoToFrontBack,
            GoForwardBackwardLayers,
            CostumeNumberName,
            BackdropNumberName,
            Size,
            HideAllSprites,
            ChangeStretchBy,
            SetStretchTo,

            // Sound
            PlaySoundUntilDone,
            PlaySound,
            StopAllSounds,
            ChangeSoundEffectBy,
            SetSoundEffectTo,
            ClearSoundEffects,
            ChangeVolumeBy,
            SetVolumeTo,
            Volume,
            SoundBeatsMenu,
            SoundEffectsMenu,

            // Events
            WhenFlagClicked,
            WhenKeyPressed,
            WhenThisSpriteClicked,
            WhenStageClicked,
            WhenBackdropSwitchesTo,
            WhenGreaterThan,
            WhenIReceive,
            WhenTouchingObject,
            Broadcast,
            BroadcastAndWait,

            // Control
            Wait,
            Repeat,
            Forever,
            If,
            IfElse,
            WaitUntil,
            RepeatUntil,
            While,
            ForEach,
            Stop,
            StartAsClone,
            CreateCloneOf,
            DeleteThisClone,
            GetCounter,
            IncrCounter,
            ClearCounter,
            AllAtOnce,

            // Sensing
            TouchingObject,
            TouchingColor,
            ColorIsTouchingColor,
            DistanceTo,
            AskAndWait,
            Answer,
            KeyPressed,
            MouseDown,
            MouseX,
            MouseY,
            SetDragMode,
            Loudness,
            Loud,
            Timer,
            ResetTimer,
            Of,
            Current,
            DaysSince2000,
            Username,
            UserId,

            // Operators
            Add,
            Subtract,
            Multiply,
            Divide,
            Random,
            GreaterThan,
            LessThan,
            Equals,
            And,
            Or,
            Not,
            Join,
            LetterOf,
            Length,
            Contains,
            Mod,
            Round,
            MathOp,

            // Variables
            Variable,
            SetVariableTo,
            ChangeVariableBy,
            ShowVariable,
            HideVariable,

            // Lists
            ListContents,
            AddToList,
            DeleteOfList,
            DeleteAllOfList,
            InsertAtList,
            ReplaceItemOfList,
            ItemOfList,
            ItemNumOfList,
            LengthOfList,
            ListContainsItem,
            ShowList,
            HideList,

            // Custom blocks
            ProceduresDefinition,
            ProceduresPrototype,
            ProceduresCall,
            ArgumentReporterStringNumber,
            ArgumentReporterBoolean,

            // Pen extension
            PenClear,
            PenStamp,
            PenDown,
            PenUp,
            PenSetColor,
            PenChangeColorParamBy,
            PenSetColorParamTo,
            PenChangeSizeBy,
            PenSetSizeTo,
            PenSetShade,
            PenChangeShadeBy,
            PenSetHue,
            PenChangeHueBy,

            // Music extension
            MusicPlayDrumForBeats,
            MusicRestForBeats,
            MusicPlayNoteForBeats,
            MusicSetInstrument,
            MusicSetTempo,
            MusicChangeTempo,
            MusicGetTempo,
            MusicMidiPlayDrumForBeats,
            MusicMidiSetInstrument,

            // Video Sensing extension
            VideoSensingWhenMotionGreaterThan,
            VideoSensingVideoOn,
            VideoSensingVideoToggle,
            VideoSensingSetVideoTransparency,

            // Text to Speech extension
            Text2SpeechSpeakAndWait,
            Text2SpeechSetVoice,
            Text2SpeechSetLanguage,

            // Translate extension
            TranslateGetTranslate,
            TranslateGetViewerLanguage,

            // Menu / shadow blocks
            MotionGotoMenu,
            MotionGlideToMenu,
            MotionPointTowardsMenu,
            LooksCostume,
            LooksBackdrops,
            SoundSoundsMenu,
            EventBroadcastMenu,
            ControlCreateCloneOfMenu,
            SensingTouchingObjectMenu,
            SensingDistanceToMenu,
            SensingKeyOptions,
            SensingOfObjectMenu,
            PenMenuColorParam,
            MusicMenuDrum,
            MusicMenuInstrument,
            MusicNote,
            VideoSensingMenuAttribute,
            VideoSensingMenuSubject,
            VideoSensingMenuVideoState,
            Text2SpeechMenuVoices,
            Text2SpeechMenuLanguages,
            TranslateMenuLanguages,
        }

        Yield,

        // Unknown or extension-specific opcode
        Other(String),
    }
}

impl BlockOpcode {
    pub fn load_json(value: &Value) -> Result<BlockOpcode, ProjectError> {
        let opcode = json_str(value, "opcode")?;
        Ok(BlockOpcode::from_str(opcode))
    }

    pub fn from_str(opcode: &str) -> BlockOpcode {
        if let Some(opcode) = BlockOpcode::known_scratch_opcode(opcode) {
            return opcode;
        }

        BlockOpcode::Other(opcode.to_string())
    }

    #[cfg(feature = "scratch")]
    fn known_scratch_opcode(opcode: &str) -> Option<BlockOpcode> {
        Some(match opcode {
            // Motion
            "motion_movesteps" => BlockOpcode::MoveSteps,
            "motion_turnright" => BlockOpcode::TurnRight,
            "motion_turnleft" => BlockOpcode::TurnLeft,
            "motion_goto" => BlockOpcode::Goto,
            "motion_gotoxy" => BlockOpcode::GotoXY,
            "motion_glideto" => BlockOpcode::GlideTo,
            "motion_glidesecstoxy" => BlockOpcode::GlideSecsToXY,
            "motion_pointindirection" => BlockOpcode::PointInDirection,
            "motion_pointtowards" => BlockOpcode::PointTowards,
            "motion_changexby" => BlockOpcode::ChangeXBy,
            "motion_setx" => BlockOpcode::SetX,
            "motion_changeyby" => BlockOpcode::ChangeYBy,
            "motion_sety" => BlockOpcode::SetY,
            "motion_ifonedgebounce" => BlockOpcode::IfOnEdgeBounce,
            "motion_setrotationstyle" => BlockOpcode::SetRotationStyle,
            "motion_xposition" => BlockOpcode::XPosition,
            "motion_yposition" => BlockOpcode::YPosition,
            "motion_direction" => BlockOpcode::Direction,
            "motion_scroll_right" => BlockOpcode::ScrollRight,
            "motion_scroll_up" => BlockOpcode::ScrollUp,
            "motion_align_scene" => BlockOpcode::AlignScene,
            "motion_xscroll" => BlockOpcode::XScroll,
            "motion_yscroll" => BlockOpcode::YScroll,

            // Looks
            "looks_sayforsecs" => BlockOpcode::SayForSecs,
            "looks_say" => BlockOpcode::Say,
            "looks_thinkforsecs" => BlockOpcode::ThinkForSecs,
            "looks_think" => BlockOpcode::Think,
            "looks_switchcostumeto" => BlockOpcode::SwitchCostumeTo,
            "looks_nextcostume" => BlockOpcode::NextCostume,
            "looks_switchbackdropto" => BlockOpcode::SwitchBackdropTo,
            "looks_switchbackdroptoandwait" => BlockOpcode::SwitchBackdropToAndWait,
            "looks_nextbackdrop" => BlockOpcode::NextBackdrop,
            "looks_changesizeby" => BlockOpcode::ChangeSizeBy,
            "looks_setsizeto" => BlockOpcode::SetSizeTo,
            "looks_changeeffectby" => BlockOpcode::ChangeEffectBy,
            "looks_seteffectto" => BlockOpcode::SetEffectTo,
            "looks_cleargraphiceffects" => BlockOpcode::ClearGraphicEffects,
            "looks_show" => BlockOpcode::Show,
            "looks_hide" => BlockOpcode::Hide,
            "looks_gotofrontback" => BlockOpcode::GoToFrontBack,
            "looks_goforwardbackwardlayers" => BlockOpcode::GoForwardBackwardLayers,
            "looks_costumenumbername" => BlockOpcode::CostumeNumberName,
            "looks_backdropnumbername" => BlockOpcode::BackdropNumberName,
            "looks_size" => BlockOpcode::Size,
            "looks_hideallsprites" => BlockOpcode::HideAllSprites,
            "looks_changestretchby" => BlockOpcode::ChangeStretchBy,
            "looks_setstretchto" => BlockOpcode::SetStretchTo,

            // Sound
            "sound_playuntildone" => BlockOpcode::PlaySoundUntilDone,
            "sound_play" => BlockOpcode::PlaySound,
            "sound_stopallsounds" => BlockOpcode::StopAllSounds,
            "sound_changeeffectby" => BlockOpcode::ChangeSoundEffectBy,
            "sound_seteffectto" => BlockOpcode::SetSoundEffectTo,
            "sound_cleareffects" => BlockOpcode::ClearSoundEffects,
            "sound_changevolumeby" => BlockOpcode::ChangeVolumeBy,
            "sound_setvolumeto" => BlockOpcode::SetVolumeTo,
            "sound_volume" => BlockOpcode::Volume,
            "sound_beats_menu" => BlockOpcode::SoundBeatsMenu,
            "sound_effects_menu" => BlockOpcode::SoundEffectsMenu,

            // Events
            "event_whenflagclicked" => BlockOpcode::WhenFlagClicked,
            "event_whenkeypressed" => BlockOpcode::WhenKeyPressed,
            "event_whenthisspriteclicked" => BlockOpcode::WhenThisSpriteClicked,
            "event_whenstageclicked" => BlockOpcode::WhenStageClicked,
            "event_whenbackdropswitchesto" => BlockOpcode::WhenBackdropSwitchesTo,
            "event_whengreaterthan" => BlockOpcode::WhenGreaterThan,
            "event_whenbroadcastreceived" => BlockOpcode::WhenIReceive,
            "event_whentouchingobject" => BlockOpcode::WhenTouchingObject,
            "event_broadcast" => BlockOpcode::Broadcast,
            "event_broadcastandwait" => BlockOpcode::BroadcastAndWait,

            // Control
            "control_wait" => BlockOpcode::Wait,
            "control_repeat" => BlockOpcode::Repeat,
            "control_forever" => BlockOpcode::Forever,
            "control_if" => BlockOpcode::If,
            "control_if_else" => BlockOpcode::IfElse,
            "control_wait_until" => BlockOpcode::WaitUntil,
            "control_repeat_until" => BlockOpcode::RepeatUntil,
            "control_while" => BlockOpcode::While,
            "control_for_each" => BlockOpcode::ForEach,
            "control_stop" => BlockOpcode::Stop,
            "control_start_as_clone" => BlockOpcode::StartAsClone,
            "control_create_clone_of" => BlockOpcode::CreateCloneOf,
            "control_delete_this_clone" => BlockOpcode::DeleteThisClone,
            "control_get_counter" => BlockOpcode::GetCounter,
            "control_incr_counter" => BlockOpcode::IncrCounter,
            "control_clear_counter" => BlockOpcode::ClearCounter,
            "control_all_at_once" => BlockOpcode::AllAtOnce,

            // Sensing
            "sensing_touchingobject" => BlockOpcode::TouchingObject,
            "sensing_touchingcolor" => BlockOpcode::TouchingColor,
            "sensing_coloristouchingcolor" => BlockOpcode::ColorIsTouchingColor,
            "sensing_distanceto" => BlockOpcode::DistanceTo,
            "sensing_askandwait" => BlockOpcode::AskAndWait,
            "sensing_answer" => BlockOpcode::Answer,
            "sensing_keypressed" => BlockOpcode::KeyPressed,
            "sensing_mousedown" => BlockOpcode::MouseDown,
            "sensing_mousex" => BlockOpcode::MouseX,
            "sensing_mousey" => BlockOpcode::MouseY,
            "sensing_setdragmode" => BlockOpcode::SetDragMode,
            "sensing_loudness" => BlockOpcode::Loudness,
            "sensing_loud" => BlockOpcode::Loud,
            "sensing_timer" => BlockOpcode::Timer,
            "sensing_resettimer" => BlockOpcode::ResetTimer,
            "sensing_of" => BlockOpcode::Of,
            "sensing_current" => BlockOpcode::Current,
            "sensing_dayssince2000" => BlockOpcode::DaysSince2000,
            "sensing_username" => BlockOpcode::Username,
            "sensing_userid" => BlockOpcode::UserId,

            // Operators
            "operator_add" => BlockOpcode::Add,
            "operator_subtract" => BlockOpcode::Subtract,
            "operator_multiply" => BlockOpcode::Multiply,
            "operator_divide" => BlockOpcode::Divide,
            "operator_random" => BlockOpcode::Random,
            "operator_gt" => BlockOpcode::GreaterThan,
            "operator_lt" => BlockOpcode::LessThan,
            "operator_equals" => BlockOpcode::Equals,
            "operator_and" => BlockOpcode::And,
            "operator_or" => BlockOpcode::Or,
            "operator_not" => BlockOpcode::Not,
            "operator_join" => BlockOpcode::Join,
            "operator_letter_of" => BlockOpcode::LetterOf,
            "operator_length" => BlockOpcode::Length,
            "operator_contains" => BlockOpcode::Contains,
            "operator_mod" => BlockOpcode::Mod,
            "operator_round" => BlockOpcode::Round,
            "operator_mathop" => BlockOpcode::MathOp,

            // Variables
            "data_variable" => BlockOpcode::Variable,
            "data_setvariableto" => BlockOpcode::SetVariableTo,
            "data_changevariableby" => BlockOpcode::ChangeVariableBy,
            "data_showvariable" => BlockOpcode::ShowVariable,
            "data_hidevariable" => BlockOpcode::HideVariable,

            // Lists
            "data_listcontents" => BlockOpcode::ListContents,
            "data_addtolist" => BlockOpcode::AddToList,
            "data_deleteoflist" => BlockOpcode::DeleteOfList,
            "data_deletealloflist" => BlockOpcode::DeleteAllOfList,
            "data_insertatlist" => BlockOpcode::InsertAtList,
            "data_replaceitemoflist" => BlockOpcode::ReplaceItemOfList,
            "data_itemoflist" => BlockOpcode::ItemOfList,
            "data_itemnumoflist" => BlockOpcode::ItemNumOfList,
            "data_lengthoflist" => BlockOpcode::LengthOfList,
            "data_listcontainsitem" => BlockOpcode::ListContainsItem,
            "data_showlist" => BlockOpcode::ShowList,
            "data_hidelist" => BlockOpcode::HideList,

            // Custom blocks
            "procedures_definition" => BlockOpcode::ProceduresDefinition,
            "procedures_prototype" => BlockOpcode::ProceduresPrototype,
            "procedures_call" => BlockOpcode::ProceduresCall,
            "argument_reporter_string_number" => BlockOpcode::ArgumentReporterStringNumber,
            "argument_reporter_boolean" => BlockOpcode::ArgumentReporterBoolean,

            // Pen extension
            "pen_clear" => BlockOpcode::PenClear,
            "pen_stamp" => BlockOpcode::PenStamp,
            "pen_penDown" => BlockOpcode::PenDown,
            "pen_penUp" => BlockOpcode::PenUp,
            "pen_setPenColorToColor" => BlockOpcode::PenSetColor,
            "pen_changePenColorParamBy" => BlockOpcode::PenChangeColorParamBy,
            "pen_setPenColorParamTo" => BlockOpcode::PenSetColorParamTo,
            "pen_changePenSizeBy" => BlockOpcode::PenChangeSizeBy,
            "pen_setPenSizeTo" => BlockOpcode::PenSetSizeTo,
            "pen_setPenShadeToNumber" => BlockOpcode::PenSetShade,
            "pen_changePenShadeBy" => BlockOpcode::PenChangeShadeBy,
            "pen_setPenHueToNumber" => BlockOpcode::PenSetHue,
            "pen_changePenHueBy" => BlockOpcode::PenChangeHueBy,

            // Music extension
            "music_playDrumForBeats" => BlockOpcode::MusicPlayDrumForBeats,
            "music_restForBeats" => BlockOpcode::MusicRestForBeats,
            "music_playNoteForBeats" => BlockOpcode::MusicPlayNoteForBeats,
            "music_setInstrument" => BlockOpcode::MusicSetInstrument,
            "music_setTempo" => BlockOpcode::MusicSetTempo,
            "music_changeTempo" => BlockOpcode::MusicChangeTempo,
            "music_getTempo" => BlockOpcode::MusicGetTempo,
            "music_midiPlayDrumForBeats" => BlockOpcode::MusicMidiPlayDrumForBeats,
            "music_midiSetInstrument" => BlockOpcode::MusicMidiSetInstrument,

            // Video Sensing extension
            "videoSensing_whenMotionGreaterThan" => BlockOpcode::VideoSensingWhenMotionGreaterThan,
            "videoSensing_videoOn" => BlockOpcode::VideoSensingVideoOn,
            "videoSensing_videoToggle" => BlockOpcode::VideoSensingVideoToggle,
            "videoSensing_setVideoTransparency" => BlockOpcode::VideoSensingSetVideoTransparency,

            // Text to Speech extension
            "text2speech_speakAndWait" => BlockOpcode::Text2SpeechSpeakAndWait,
            "text2speech_setVoice" => BlockOpcode::Text2SpeechSetVoice,
            "text2speech_setLanguage" => BlockOpcode::Text2SpeechSetLanguage,

            // Translate extension
            "translate_getTranslate" => BlockOpcode::TranslateGetTranslate,
            "translate_getViewerLanguage" => BlockOpcode::TranslateGetViewerLanguage,

            // Menu / shadow blocks
            "motion_goto_menu" => BlockOpcode::MotionGotoMenu,
            "motion_glideto_menu" => BlockOpcode::MotionGlideToMenu,
            "motion_pointtowards_menu" => BlockOpcode::MotionPointTowardsMenu,
            "looks_costume" => BlockOpcode::LooksCostume,
            "looks_backdrops" => BlockOpcode::LooksBackdrops,
            "sound_sounds_menu" => BlockOpcode::SoundSoundsMenu,
            "event_broadcast_menu" => BlockOpcode::EventBroadcastMenu,
            "control_create_clone_of_menu" => BlockOpcode::ControlCreateCloneOfMenu,
            "sensing_touchingobjectmenu" => BlockOpcode::SensingTouchingObjectMenu,
            "sensing_distancetomenu" => BlockOpcode::SensingDistanceToMenu,
            "sensing_keyoptions" => BlockOpcode::SensingKeyOptions,
            "sensing_of_object_menu" => BlockOpcode::SensingOfObjectMenu,
            "pen_menu_colorParam" => BlockOpcode::PenMenuColorParam,
            "music_menu_DRUM" => BlockOpcode::MusicMenuDrum,
            "music_menu_INSTRUMENT" => BlockOpcode::MusicMenuInstrument,
            "note" => BlockOpcode::MusicNote,
            "videoSensing_menu_ATTRIBUTE" => BlockOpcode::VideoSensingMenuAttribute,
            "videoSensing_menu_SUBJECT" => BlockOpcode::VideoSensingMenuSubject,
            "videoSensing_menu_VIDEO_STATE" => BlockOpcode::VideoSensingMenuVideoState,
            "text2speech_menu_voices" => BlockOpcode::Text2SpeechMenuVoices,
            "text2speech_menu_languages" => BlockOpcode::Text2SpeechMenuLanguages,
            "translate_menu_languages" => BlockOpcode::TranslateMenuLanguages,

            _ => return None,
        })
    }

    #[cfg(not(feature = "scratch"))]
    fn known_scratch_opcode(_opcode: &str) -> Option<BlockOpcode> {
        None
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_inputs() {
        let block = Block::load_json(
            "block".to_string(),
            &json!({
                "opcode": "looks_say",
                "next": null,
                "parent": null,
                "inputs": {
                    "SHADOW": [1, [10, "Hello!"]],
                    "BLOCK": [2, "reporter"],
                    "OBSCURED": [3, "reporter", [4, 10]]
                },
                "fields": {}
            }),
        )
        .unwrap();

        assert_eq!(
            block.inputs["SHADOW"],
            Input::Shadow(InputValue::Primitive(Primitive::Text("Hello!".to_string())))
        );
        assert_eq!(
            block.inputs["BLOCK"],
            Input::NoShadow(InputValue::Block("reporter".to_string()))
        );
        assert_eq!(
            block.inputs["OBSCURED"],
            Input::Obscured {
                value: InputValue::Block("reporter".to_string()),
                shadow: InputValue::Primitive(Primitive::Number(10.0)),
            }
        );
    }

    #[test]
    fn parses_fields() {
        let block = Block::load_json(
            "block".to_string(),
            &json!({
                "opcode": "data_setvariableto",
                "next": null,
                "parent": null,
                "inputs": {},
                "fields": {
                    "VARIABLE": ["my variable", "variable-id"],
                    "STYLE": ["left-right", null]
                }
            }),
        )
        .unwrap();

        assert_eq!(
            block.fields["VARIABLE"],
            Field {
                value: RuntimeValue::String("my variable".to_string()),
                id: Some("variable-id".to_string()),
            }
        );
        assert_eq!(
            block.fields["STYLE"],
            Field {
                value: RuntimeValue::String("left-right".to_string()),
                id: None,
            }
        );
    }

    #[test]
    fn parses_variable_and_list_primitives() {
        let block = Block::load_json(
            "block".to_string(),
            &json!({
                "opcode": "looks_say",
                "next": null,
                "parent": null,
                "inputs": {
                    "VARIABLE": [1, [12, "my variable", "variable-id"]],
                    "LIST": [1, [13, "my list", "list-id"]],
                    "BROADCAST": [1, [11, "message1", "broadcast-id"]]
                },
                "fields": {}
            }),
        )
        .unwrap();

        assert_eq!(
            block.inputs["VARIABLE"],
            Input::Shadow(InputValue::Primitive(Primitive::Variable {
                name: "my variable".to_string(),
                id: "variable-id".to_string(),
            }))
        );
        assert_eq!(
            block.inputs["LIST"],
            Input::Shadow(InputValue::Primitive(Primitive::List {
                name: "my list".to_string(),
                id: "list-id".to_string(),
            }))
        );
        assert_eq!(
            block.inputs["BROADCAST"],
            Input::Shadow(InputValue::Primitive(Primitive::Broadcast {
                name: "message1".to_string(),
                id: "broadcast-id".to_string(),
            }))
        );
    }
}
