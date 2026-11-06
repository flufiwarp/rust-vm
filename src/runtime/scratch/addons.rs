//! Scratch Addons debugger custom procedures, using their exact registered IDs.
//! See ScratchAddons/ScratchAddons addons/debugger/userscript.js.

use super::argument_keys;
use crate::runtime::thread::PendingKind;
use crate::runtime::{Effect, Request, RequestId, Response, Runtime, RuntimeError, VmThread};
use crate::{Instruction, Value};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebuggerLogLevel {
    Log,
    Warn,
    Error,
}

impl std::fmt::Display for DebuggerLogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Log => "log",
            Self::Warn => "warn",
            Self::Error => "error",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DebuggerEvent {
    Log {
        level: DebuggerLogLevel,
        message: String,
    },
    Breakpoint,
    TimerStarted {
        label: String,
    },
    TimerStopped {
        label: String,
        elapsed: Duration,
    },
}

#[derive(Default)]
pub(crate) struct DebuggerState {
    pub timers: HashMap<String, Duration>,
    pub paused_request: Option<RequestId>,
}

impl Runtime {
    pub fn is_debugger_paused(&self) -> bool {
        self.debugger.paused_request.is_some()
    }

    /// Completing the pause request directly via respond has the same effect.
    pub fn resume_debugger(&mut self) -> Result<(), RuntimeError> {
        if let Some(id) = self.debugger.paused_request {
            self.respond(id, Response::Completed)?;
        }
        Ok(())
    }

    // None means an ordinary custom procedure; Some(true) means its result is
    // deferred to a request. Match exact IDs, never strip invisible characters
    // from arbitrary user procedures or intercept an ordinary block named log.
    pub(crate) fn call_debugger_procedure(
        &mut self,
        thread: &mut VmThread,
        code: &str,
        mutation: &serde_json::Value,
        arguments: &HashMap<String, Instruction>,
        values: &[Value],
        discard: bool,
    ) -> Option<bool> {
        let event = match code {
            "\u{200b}\u{200b}log\u{200b}\u{200b} %s"
            | "\u{200b}\u{200b}warn\u{200b}\u{200b} %s"
            | "\u{200b}\u{200b}error\u{200b}\u{200b} %s" => {
                let level = if code.contains("warn") {
                    DebuggerLogLevel::Warn
                } else if code.contains("error") {
                    DebuggerLogLevel::Error
                } else {
                    DebuggerLogLevel::Log
                };
                DebuggerEvent::Log {
                    level,
                    message: first_argument(mutation, arguments, values),
                }
            }
            "\u{200b}\u{200b}start timer\u{200b}\u{200b} %s" => {
                let label = first_argument(mutation, arguments, values);
                // Starting an active label restarts it, as in Scratch Addons.
                self.debugger.timers.insert(label.clone(), self.clock);
                DebuggerEvent::TimerStarted { label }
            }
            "\u{200b}\u{200b}stop timer\u{200b}\u{200b} %s" => {
                let label = first_argument(mutation, arguments, values);
                let Some(start) = self.debugger.timers.remove(&label) else {
                    return Some(false);
                };
                DebuggerEvent::TimerStopped {
                    label,
                    elapsed: self.clock.saturating_sub(start),
                }
            }
            "\u{200b}\u{200b}breakpoint\u{200b}\u{200b}" | "sa-pause" => {
                self.effects.push_back(Effect::Debugger {
                    target_id: thread.target_id,
                    thread_id: thread.id,
                    event: DebuggerEvent::Breakpoint,
                });
                self.send_request(thread, Request::DebuggerPause, PendingKind::Value, discard);
                self.debugger.paused_request = thread.pending.as_ref().map(|pending| pending.id);
                // Do not include time spent in the debugger in future ticks.
                self.last_tick = Instant::now();
                return Some(true);
            }
            _ => return None,
        };
        self.effects.push_back(Effect::Debugger {
            target_id: thread.target_id,
            thread_id: thread.id,
            event,
        });
        Some(false)
    }
}

fn first_argument(
    mutation: &serde_json::Value,
    arguments: &HashMap<String, Instruction>,
    values: &[Value],
) -> String {
    let keys = argument_keys(mutation, arguments);
    let index = keys.iter().position(|key| key == "arg0").unwrap_or(0);
    values.get(index).cloned().unwrap_or_default().to_string()
}
