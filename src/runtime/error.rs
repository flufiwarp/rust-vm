use thiserror::Error;

use crate::{BlockId, BlockOpcode, ProjectError, cfg_variants, project::TargetId};

cfg_variants! {
#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("target not found {0}")]
    TargetNotFound(TargetId),
    #[error("block not found {0}")]
    BlockNotFound(BlockId),
    #[error("thread has no stackframe")]
    NoStackFrame,
    #[error("invalid instruction state: {0}")]
    InvalidInstruction(String),
    #[error("unknown or already consumed request {0}")]
    UnknownRequest(u64),
    #[error("request {0} already has a response")]
    AlreadyResponded(u64),
    #[error("response type does not match request {0}")]
    InvalidResponse(u64),
    #[error("request {id} failed: {message}")]
    RequestFailed { id: u64, message: String },
    #[cfg(feature = "scratch")] {
        #[error("procedure not found: {0}")]
        ProcedureNotFound(String),
        #[error("procedure call depth exceeded")]
        CallDepthExceeded,
    }

    #[error("project error: {0}")]
    ProjectError(#[from] ProjectError),
    #[error("compile error: {0}")]
    CompileError(#[from] CompileError),
}
}

#[derive(Error, Debug)]
pub enum CompileError {
    #[error("target not found {0}")]
    TargetNotFound(TargetId),
    #[error("block not found {0}")]
    BlockNotFound(BlockId),
    #[error("unsupported block opcode {0:?}")]
    UnsupportedOpcode(BlockOpcode),
    #[error("block {block} is missing input {input}")]
    MissingInput { block: BlockId, input: String },
    #[error("block {block} is missing field {field}")]
    MissingField { block: BlockId, field: String },
    #[error("block {block} input {input} must reference a substack")]
    InvalidSubstack { block: BlockId, input: String },
    #[error("block {0} is missing its procedure mutation")]
    MissingMutation(BlockId),
    #[error("cyclic block reference at {0}")]
    CyclicBlock(BlockId),

    #[error("project error: {0}")]
    ProjectError(#[from] ProjectError),
}
