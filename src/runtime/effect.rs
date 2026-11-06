#[cfg(feature = "scratch-addons-debugger")]
use super::DebuggerEvent;
use super::{Request, RequestId, ThreadId};
use crate::{TargetId, cfg_variants};

cfg_variants! {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Effect {
        /// Emitted once. The owning thread resumes after Runtime::respond.
        Request { id: RequestId, target_id: TargetId, thread_id: ThreadId, request: Request },
        /// The frontend should dismiss/cancel an outstanding operation.
        CancelRequest { id: RequestId },
        Warning { target_id: TargetId, thread_id: ThreadId, message: String },

        #[cfg(feature = "scratch-addons-debugger")] {
            Debugger { target_id: TargetId, thread_id: ThreadId, event: DebuggerEvent },
        }

        #[cfg(feature = "scratch")] {
            TargetChanged { target_id: TargetId, state: TargetSnapshot },
            Bubble { target_id: TargetId, kind: BubbleKind, text: Option<String> },
            PlaySound { target_id: TargetId, sound: String },
            StopAllSounds,
            GraphicEffect { target_id: TargetId, effect: String, value: f64 },
            ClearGraphicEffects { target_id: TargetId },
            SoundEffect { target_id: TargetId, effect: String, value: f64 },
            ClearSoundEffects { target_id: TargetId },
            Monitor { target_id: TargetId, id: String, is_list: bool, visible: bool },
            DataChanged { target_id: TargetId, id: String, is_list: bool },
            Pen { target_id: TargetId, command: PenCommand },
            Music { target_id: TargetId, command: MusicCommand },
            Video(VideoCommand),
            CloneCreated { source_id: TargetId, target_id: TargetId, state: TargetSnapshot },
            CloneDeleted { target_id: TargetId },
        }
    }
}

/// Snapshots preserve intermediate movement/volume changes even if the frontend
/// drains multiple effects after a tick (important for drawing pen strokes).
#[cfg(feature = "scratch")]
#[derive(Debug, Clone, PartialEq)]
pub enum TargetSnapshot {
    Stage(crate::StageTargetState),
    Sprite(crate::NormalTargetState),
}

#[cfg(feature = "scratch")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleKind {
    Say,
    Think,
}

#[cfg(feature = "scratch")]
#[derive(Debug, Clone, PartialEq)]
pub enum PenCommand {
    Clear,
    Stamp,
    Down,
    Up,
    SetColor(String),
    ChangeColorParam {
        parameter: String,
        value: f64,
    },
    SetColorParam {
        parameter: String,
        value: f64,
    },
    ChangeSize(f64),
    SetSize(f64),
    /// Legacy Scratch 2 hue/shade have different units from color parameters.
    SetHue(f64),
    ChangeHue(f64),
    SetShade(f64),
    ChangeShade(f64),
}

#[cfg(feature = "scratch")]
#[derive(Debug, Clone, PartialEq)]
pub enum MusicCommand {
    Drum { drum: f64, beats: f64, tempo: f64 },
    Note { note: f64, beats: f64, tempo: f64 },
    SetInstrument(f64),
    SetTempo(f64),
}

#[cfg(feature = "scratch")]
#[derive(Debug, Clone, PartialEq)]
pub enum VideoCommand {
    Toggle(String),
    SetTransparency(f64),
}
