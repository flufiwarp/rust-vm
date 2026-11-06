use crate::{BlockId, BlockOpcode, Project, TargetId};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

mod effect;
mod error;
mod executor;
mod request;
mod scheduler;
mod thread;
#[macro_use]
mod logging;
mod compiler;
mod instruction;
#[cfg(feature = "scratch")]
mod scratch;
#[cfg(test)]
mod tests;

pub use compiler::compile_block;
pub use effect::Effect;
#[cfg(feature = "scratch")]
pub use effect::{BubbleKind, MusicCommand, PenCommand, TargetSnapshot, VideoCommand};
pub use error::{CompileError, RuntimeError};
pub use executor::ThreadFlowResult;
pub use instruction::Instruction;
#[cfg(feature = "scratch")]
pub use request::Sensor;
pub use request::{Request, RequestId, Response};
pub use scheduler::{DefaultScheduler, Scheduler};
#[cfg(feature = "scratch-addons-debugger")]
pub use scratch::{DebuggerEvent, DebuggerLogLevel};
#[cfg(feature = "scratch")]
pub use scratch::{ScratchState, ScratchTargetState};
pub use thread::{ThreadId, ThreadStatus, VmThread};

#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    /// Hide diagnostic/effect logs in the CLI frontend, preserving debugger
    /// output and say/think text. Does not suppress effects or returned errors.
    pub quiet_logs: bool,
    /// Missing calls always emit a warning. By default they are skipped; turn
    /// this on to finish their thread with ProcedureNotFound instead.
    #[cfg(feature = "scratch")]
    pub error_on_missing_procedures: bool,
}

#[derive(Debug)]
pub struct PendingRequest {
    pub target_id: TargetId,
    pub thread_id: ThreadId,
    pub request: Request,
    response: Option<Response>,
}

pub struct Runtime {
    pub project: Box<Project>,
    pub config: RuntimeConfig,
    pub scheduler: Box<dyn Scheduler>,
    /// A hard per-thread task budget; even empty/warp loops cannot hang a tick.
    pub tasks_per_tick: usize,
    effects: VecDeque<Effect>,
    requests: HashMap<RequestId, PendingRequest>,
    next_request_id: RequestId,
    clock: Duration,
    last_tick: Instant,
    #[cfg(feature = "scratch")]
    pub scratch: ScratchState,
    #[cfg(feature = "scratch-addons-debugger")]
    debugger: scratch::DebuggerState,
}

impl Runtime {
    pub fn new(project: Project) -> Self {
        Self::with_config(project, RuntimeConfig::default())
    }

    pub fn with_config(project: Project, config: RuntimeConfig) -> Self {
        Self {
            config,
            #[cfg(feature = "scratch-addons-debugger")]
            debugger: scratch::DebuggerState::default(),
            #[cfg(feature = "scratch")]
            scratch: ScratchState::new(&project),
            project: Box::new(project),
            scheduler: Box::new(DefaultScheduler::new()),
            tasks_per_tick: 1000,
            effects: VecDeque::new(),
            requests: HashMap::new(),
            next_request_id: 1,
            clock: Duration::ZERO,
            last_tick: Instant::now(),
        }
    }

    /// Advances using wall-clock elapsed time. No frontend operation is awaited
    /// inside this future: waiting threads are polled once on subsequent ticks.
    pub async fn tick(&mut self) -> Result<(), RuntimeError> {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        self.advance(delta)
    }

    /// Deterministic alternative for game loops, tests, or simulated time.
    pub async fn tick_with_delta(&mut self, delta: Duration) -> Result<(), RuntimeError> {
        self.last_tick = Instant::now();
        self.advance(delta)
    }

    fn advance(&mut self, delta: Duration) -> Result<(), RuntimeError> {
        let orphaned: Vec<_> = self
            .requests
            .iter()
            .filter(|(_, request)| self.scheduler.thread(request.thread_id).is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in orphaned {
            self.cancel_request(id);
        }
        #[cfg(feature = "scratch-addons-debugger")]
        if self.is_debugger_paused() {
            return Ok(());
        }
        self.clock = self.clock.saturating_add(delta);
        let mut error = None;
        // Snapshotting prevents newly spawned scripts from starving older ones.
        for id in self.scheduler.thread_ids() {
            let Some(mut thread) = self.scheduler.take_thread(id) else {
                continue;
            };
            if let Err(cause) = self.step_thread(&mut thread) {
                thread.status = ThreadStatus::Finished;
                if error.is_none() {
                    error = Some(cause);
                }
            }
            if thread.status != ThreadStatus::Finished {
                self.scheduler.add_thread(thread);
            } else {
                self.cancel_thread_requests(id);
            }
            #[cfg(feature = "scratch-addons-debugger")]
            if self.is_debugger_paused() {
                break;
            }
        }
        if let Some(error) = error {
            Err(error)
        } else {
            Ok(())
        }
    }

    pub fn time(&self) -> Duration {
        self.clock
    }
    pub fn is_idle(&self) -> bool {
        self.scheduler.thread_ids().is_empty()
    }
    pub fn drain_effects(&mut self) -> Vec<Effect> {
        self.effects.drain(..).collect()
    }
    pub fn pending_request(&self, id: RequestId) -> Option<&PendingRequest> {
        self.requests.get(&id)
    }
    pub fn pending_requests(&self) -> impl Iterator<Item = (RequestId, &PendingRequest)> {
        self.requests.iter().map(|(id, request)| (*id, request))
    }

    /// Records a response; execution resumes on the next tick. Wrong response
    /// types leave the request pending, so a frontend can correct its response.
    pub fn respond(&mut self, id: RequestId, response: Response) -> Result<(), RuntimeError> {
        let request = self
            .requests
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownRequest(id))?;
        if request.response.is_some() {
            return Err(RuntimeError::AlreadyResponded(id));
        }
        if !request.request.accepts(&response) {
            return Err(RuntimeError::InvalidResponse(id));
        }
        request.response = Some(response);
        #[cfg(feature = "scratch-addons-debugger")]
        if self.debugger.paused_request == Some(id) {
            self.debugger.paused_request = None;
            self.last_tick = Instant::now();
        }
        Ok(())
    }

    /// Queues a new script for the next tick and returns its cancellation ID.
    pub fn run_instructions(
        &mut self,
        target_id: &TargetId,
        instructions: Vec<Instruction>,
    ) -> Result<ThreadId, RuntimeError> {
        if !self.project.targets.contains_key(target_id) {
            return Err(RuntimeError::TargetNotFound(*target_id));
        }
        let thread = VmThread::new(*target_id, instructions);
        let id = thread.id;
        self.scheduler.add_thread(thread);
        Ok(id)
    }

    pub async fn run_block(
        &mut self,
        target_id: &TargetId,
        block_id: &BlockId,
    ) -> Result<(), RuntimeError> {
        let instructions = compile_block(&self.project, block_id, target_id)?;
        self.run_instructions(target_id, instructions)?;
        Ok(())
    }

    fn start_hat(
        &mut self,
        target_id: &TargetId,
        block_id: &BlockId,
    ) -> Result<Option<ThreadId>, RuntimeError> {
        let target = self
            .project
            .targets
            .get(target_id)
            .ok_or(RuntimeError::TargetNotFound(*target_id))?;
        let block = target
            .sprite()
            .blocks
            .get(block_id)
            .ok_or_else(|| RuntimeError::BlockNotFound(block_id.clone()))?;
        let Some(next) = &block.next else {
            return Ok(None);
        };
        let instructions = compile_block(&self.project, next, target_id)?;
        self.run_instructions(target_id, instructions).map(Some)
    }

    pub async fn run_hat(
        &mut self,
        target_id: &TargetId,
        block_id: &BlockId,
    ) -> Result<(), RuntimeError> {
        self.start_hat(target_id, block_id)?;
        Ok(())
    }

    pub async fn run_target_hats(
        &mut self,
        target_id: &TargetId,
        opcode: &BlockOpcode,
    ) -> Result<(), RuntimeError> {
        let target = self
            .project
            .targets
            .get(target_id)
            .ok_or(RuntimeError::TargetNotFound(*target_id))?;
        let ids = target.sprite().find_top_level(opcode);
        for id in ids {
            self.start_hat(target_id, &id)?;
        }
        Ok(())
    }

    pub async fn run_hats(&mut self, opcode: &BlockOpcode) -> Result<(), RuntimeError> {
        let mut ids: Vec<_> = self.project.targets.keys().copied().collect();
        ids.sort();
        for id in ids {
            self.run_target_hats(&id, opcode).await?;
        }
        Ok(())
    }

    pub async fn run_thread(&mut self, thread: &mut VmThread) -> Result<(), RuntimeError> {
        let result = self.step_thread(thread);
        if result.is_err() {
            thread.status = ThreadStatus::Finished;
        }
        if thread.status == ThreadStatus::Finished {
            self.cancel_thread_requests(thread.id);
        }
        result.map(|_| ())
    }

    pub fn stop_thread(&mut self, id: ThreadId) {
        self.scheduler.stop_thread(id);
        self.cancel_thread_requests(id);
    }

    pub fn stop_all(&mut self) {
        self.scheduler.stop_all();
        let ids: Vec<_> = self.requests.keys().copied().collect();
        for id in ids {
            self.cancel_request(id);
        }
        #[cfg(feature = "scratch")]
        self.effects.push_back(Effect::StopAllSounds);
    }

    fn cancel_request(&mut self, id: RequestId) {
        #[cfg(feature = "scratch-addons-debugger")]
        if self.debugger.paused_request == Some(id) {
            self.debugger.paused_request = None;
            self.last_tick = Instant::now();
        }
        if self.requests.remove(&id).is_some() {
            self.effects.push_back(Effect::CancelRequest { id });
        }
    }

    fn cancel_thread_requests(&mut self, thread: ThreadId) {
        let ids: Vec<_> = self
            .requests
            .iter()
            .filter(|(_, request)| request.thread_id == thread)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.cancel_request(id);
        }
    }
}
