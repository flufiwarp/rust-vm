#![recursion_limit = "1024"]

mod macros;
mod project;
mod runtime;
mod value;

#[cfg(feature = "cli-frontend")]
pub mod cli_frontend;
#[cfg(feature = "cli-frontend")]
pub use cli_frontend::{CliFrontend, CliFrontendError};

pub use project::{
    Block, BlockId, BlockOpcode, Field, Input, InputValue, NormalTargetState, Primitive, Project,
    ProjectError, RotationStyle, Sprite, SpriteId, StageTargetState, Target, TargetId,
};

pub use runtime::{
    CompileError, DefaultScheduler, Effect, Instruction, PendingRequest, Request, RequestId,
    Response, Runtime, RuntimeConfig, RuntimeError, Scheduler, ThreadFlowResult, ThreadId,
    ThreadStatus, VmThread, compile_block,
};

#[cfg(feature = "scratch")]
pub use project::{ScratchAsset, ScratchList, ScratchSpriteData, ScratchVariable};
#[cfg(feature = "scratch")]
pub use runtime::{
    BubbleKind, MusicCommand, PenCommand, ScratchState, ScratchTargetState, Sensor, TargetSnapshot,
    VideoCommand,
};

pub use value::Value;

#[cfg(feature = "scratch-addons-debugger")]
pub use runtime::{DebuggerEvent, DebuggerLogLevel};
