use super::RequestId;
use crate::{Instruction, TargetId, Value, cfg_variants};
use std::time::Duration;

#[cfg(feature = "scratch")]
use super::BubbleKind;
#[cfg(feature = "scratch")]
use crate::Field;

pub type ThreadId = u64;

#[derive(Debug, Clone, PartialEq)]
pub enum ThreadStatus {
    Running,
    Sleeping { until: Duration },
    Waiting { request_id: RequestId },
    Joining { threads: Vec<ThreadId> },
    Finished,
}

cfg_variants! {
    #[derive(Debug, Clone)]
    pub(crate) enum Task {
        Eval { instruction: Instruction, discard: bool },
        Apply { instruction: Instruction, count: usize, discard: bool },
        Forever { body: Vec<Instruction> },
        Yield,
        #[cfg(feature = "scratch")] {
            Repeat { body: Vec<Instruction>, remaining: f64 },
            ForEach { variable: Field, value: Box<Instruction>, body: Vec<Instruction>, index: f64 },
            ForEachStep { variable: Field, value: Box<Instruction>, body: Vec<Instruction>, index: f64 },
            EndProcedure { warp: bool },
            ClearBubble { kind: BubbleKind, generation: u64 },
            Glide { from: (f64, f64), to: (f64, f64), start: Duration, duration: Duration },
        }
    }
}

cfg_variants! {
    #[derive(Debug, Clone)]
    pub(crate) enum PendingKind {
        Value,
        #[cfg(feature = "scratch")] {
            Answer,
            Position(PositionAction),
            Motion,
            Distance,
            Loud,
        }
    }
}

#[cfg(feature = "scratch")]
#[derive(Debug, Clone)]
pub(crate) enum PositionAction {
    Goto,
    Point,
    Glide(f64),
}

#[derive(Debug)]
pub(crate) struct Pending {
    pub id: RequestId,
    pub kind: PendingKind,
    pub discard: bool,
}

#[derive(Debug)]
pub struct VmThread {
    pub id: ThreadId,
    pub target_id: TargetId,
    pub status: ThreadStatus,
    pub(crate) tasks: Vec<Task>,
    pub(crate) values: Vec<Value>,
    pub(crate) pending: Option<Pending>,
    pub(crate) yielded: bool,
    #[cfg(feature = "scratch")]
    pub(crate) arguments: Vec<std::collections::HashMap<String, Value>>,
    #[cfg(feature = "scratch")]
    pub(crate) warp_depth: usize,
}

impl VmThread {
    pub fn new(target_id: TargetId, instructions: Vec<Instruction>) -> Self {
        let mut thread = Self {
            id: rand::random(),
            target_id,
            status: ThreadStatus::Running,
            tasks: Vec::new(),
            values: Vec::new(),
            pending: None,
            yielded: false,
            #[cfg(feature = "scratch")]
            arguments: Vec::new(),
            #[cfg(feature = "scratch")]
            warp_depth: 0,
        };
        thread.push_stack(instructions);
        thread
    }

    pub(crate) fn push_stack(&mut self, instructions: Vec<Instruction>) {
        self.tasks.extend(
            instructions
                .into_iter()
                .rev()
                .map(|instruction| Task::Eval {
                    instruction,
                    discard: true,
                }),
        );
    }

    pub(crate) fn loop_yield(&mut self) {
        #[cfg(feature = "scratch")]
        if self.warp_depth > 0 {
            return;
        }
        self.tasks.push(Task::Yield);
    }

    #[cfg(feature = "scratch")]
    pub(crate) fn stop_this_script(&mut self) {
        // Scratch's stop-this-script unwinds to the nearest procedures_call.
        // Keep its cleanup task so arguments and warp depth are restored before
        // executing the caller's continuation. Outside a procedure it stops all.
        if let Some(frame) = self
            .tasks
            .iter()
            .rposition(|task| matches!(task, Task::EndProcedure { .. }))
        {
            self.tasks.truncate(frame + 1);
        } else {
            self.status = ThreadStatus::Finished;
        }
    }
}
