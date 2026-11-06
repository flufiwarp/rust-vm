use super::{ThreadId, ThreadStatus, VmThread};
use crate::TargetId;

/// The runtime takes each thread out while stepping it, avoiding aliased mutable
/// references to the scheduler and runtime. New threads start on the next tick.
pub trait Scheduler {
    fn add_thread(&mut self, thread: VmThread);
    fn take_thread(&mut self, id: ThreadId) -> Option<VmThread>;
    fn thread_ids(&self) -> Vec<ThreadId>;
    fn thread(&self, id: ThreadId) -> Option<&VmThread>;
    fn stop_thread(&mut self, id: ThreadId) {
        self.take_thread(id);
    }
    fn stop_all(&mut self) {
        for id in self.thread_ids() {
            self.stop_thread(id);
        }
    }
    fn stop_target(&mut self, target: TargetId) {
        for id in self.thread_ids() {
            if self
                .thread(id)
                .is_some_and(|thread| thread.target_id == target)
            {
                self.stop_thread(id);
            }
        }
    }
    fn cleanup_threads(&mut self) {
        for id in self.thread_ids() {
            if self
                .thread(id)
                .is_some_and(|thread| thread.status == ThreadStatus::Finished)
            {
                self.stop_thread(id);
            }
        }
    }
}

#[derive(Default)]
pub struct DefaultScheduler {
    threads: Vec<VmThread>,
}

impl DefaultScheduler {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Scheduler for DefaultScheduler {
    fn add_thread(&mut self, thread: VmThread) {
        self.threads.push(thread);
    }
    fn take_thread(&mut self, id: ThreadId) -> Option<VmThread> {
        let index = self.threads.iter().position(|thread| thread.id == id)?;
        Some(self.threads.remove(index))
    }
    fn thread_ids(&self) -> Vec<ThreadId> {
        self.threads.iter().map(|thread| thread.id).collect()
    }
    fn thread(&self, id: ThreadId) -> Option<&VmThread> {
        self.threads.iter().find(|thread| thread.id == id)
    }
}
