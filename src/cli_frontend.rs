//! Console effect logging and nonblocking Scratch `ask` handling.
//!
//! Enable `cli-frontend` to use this module and `scratch` for Scratch-specific
//! handling. The async run loop requires a Tokio runtime with time enabled.

use crate::{Effect, Response, Runtime, RuntimeError, cfg_variants};
use std::{
    io::{self, BufRead, Write},
    time::Duration,
};

#[cfg(feature = "scratch")]
use crate::{Request, RequestId};
#[cfg(feature = "scratch")]
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum CliFrontendError {
    #[error("console I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("runtime error: {0}")]
    Runtime(#[from] RuntimeError),
}

#[cfg(feature = "scratch")]
type Output = Arc<Mutex<Box<dyn Write + Send>>>;

/// Logs all effects to stderr and prints bubbles/prompts to stdout by default.
/// Set `runtime.config.quiet_logs` to keep only debugger and say/think output.
/// Input prompts and returned errors are unaffected.
pub struct CliFrontend {
    pub frame_interval: Duration,
    /// None uses wall-clock time. Set a fixed delta for deterministic/headless
    /// runs; combine with a zero frame_interval to simulate frames quickly.
    pub frame_delta: Option<Duration>,
    /// Opt-in Scratch-without-a-renderer/input behavior, not graphical parity.
    /// Unsupported requests still fail unless covered by these explicit defaults.
    #[cfg(feature = "scratch")]
    pub headless: bool,
    #[cfg(feature = "scratch")]
    pub headless_fallbacks: u64,
    log: Box<dyn Write + Send>,
    #[cfg(feature = "scratch")]
    output: Output,
    #[cfg(feature = "scratch")]
    input: Option<Box<dyn BufRead + Send>>,
    #[cfg(feature = "scratch")]
    worker: Option<InputWorker>,
    #[cfg(feature = "scratch")]
    questions: HashMap<RequestId, PendingInput>,
}

impl Default for CliFrontend {
    fn default() -> Self {
        Self::new()
    }
}

impl CliFrontend {
    pub fn new() -> Self {
        Self::with_io(io::BufReader::new(io::stdin()), io::stdout(), io::stderr())
    }

    /// Supplies alternate input, bubble/prompt output, and effect-log streams.
    /// Input is read by one lazily started worker, never by the VM tick loop.
    pub fn with_io(
        input: impl BufRead + Send + 'static,
        output: impl Write + Send + 'static,
        log: impl Write + Send + 'static,
    ) -> Self {
        #[cfg(not(feature = "scratch"))]
        let _ = (input, output);
        Self {
            frame_interval: Duration::from_millis(16),
            frame_delta: None,
            #[cfg(feature = "scratch")]
            headless: false,
            #[cfg(feature = "scratch")]
            headless_fallbacks: 0,
            log: Box::new(io::BufWriter::new(log)),
            #[cfg(feature = "scratch")]
            output: Arc::new(Mutex::new(Box::new(output))),
            #[cfg(feature = "scratch")]
            input: Some(Box::new(input)),
            #[cfg(feature = "scratch")]
            worker: None,
            #[cfg(feature = "scratch")]
            questions: HashMap::new(),
        }
    }

    /// Runs already scheduled scripts until idle or error. This does not start
    /// hats or reset the project. Sleeping between frames yields to Tokio.
    pub async fn run(&mut self, runtime: &mut Runtime) -> Result<(), CliFrontendError> {
        let result = async {
            loop {
                self.tick(runtime).await?;
                if runtime.is_idle() {
                    break;
                }
                if self.frame_interval.is_zero() {
                    tokio::task::yield_now().await;
                } else {
                    tokio::time::sleep(self.frame_interval).await;
                }
            }
            Ok(())
        }
        .await;
        #[cfg(feature = "scratch")]
        self.cancel_questions();
        result
    }

    #[cfg(feature = "scratch")]
    pub async fn run_green_flag(&mut self, runtime: &mut Runtime) -> Result<(), CliFrontendError> {
        let result = runtime.green_flag().await;
        self.process_effects(runtime)?;
        result?;
        self.run(runtime).await
    }

    /// Advances the VM once, logs/handles its effects, and polls input replies.
    /// Effects emitted during a failed VM tick are still logged before returning.
    pub async fn tick(&mut self, runtime: &mut Runtime) -> Result<(), CliFrontendError> {
        // Replies received between frames should resume on this tick, not wait
        // for an extra frame. process_effects also picks up newly ready replies.
        #[cfg(feature = "scratch")]
        self.poll_answers(runtime)?;
        let result = if let Some(delta) = self.frame_delta {
            runtime.tick_with_delta(delta).await
        } else {
            runtime.tick().await
        };
        self.process_effects(runtime)?;
        result?;
        Ok(())
    }

    /// For callers with their own clock/loop: drain effects and poll ready input
    /// replies without advancing the runtime or waiting for user input.
    pub fn process_effects(&mut self, runtime: &mut Runtime) -> Result<(), CliFrontendError> {
        for effect in runtime.drain_effects() {
            cfg_variants! {
                match (&effect) {
                    #[cfg(feature = "scratch")] {
                        Effect::Request { id, request: Request::Ask { question }, .. } => {
                            self.log_effect(runtime, &effect, true)?;
                            // A request may have been cancelled later in the same tick.
                            if runtime.pending_request(*id).is_some() {
                                self.ask(*id, question.clone(), InputKind::Answer)?;
                            }
                        },
                        Effect::Request { id, request: Request::Bounce { x, y, direction }, .. } => {
                            // There is no renderer here to measure costume bounds.
                            // Explicitly label the point-sprite stage-edge fallback.
                            self.log_labeled_effect(runtime, &effect, "effect: approximate stage bounds")?;
                            Self::respond(runtime, *id, Self::bounce_response(*x, *y, *direction))?;
                        },
                        Effect::Request { id, request: Request::Sense(sensor), .. } => {
                            if let Some(response) = Self::clock_sensor(sensor) {
                                self.log_effect(runtime, &effect, true)?;
                                Self::respond(runtime, *id, response)?;
                            } else if self.headless {
                                if let Some(response) = Self::headless_sensor(sensor) {
                                    self.headless_fallbacks += 1;
                                    self.log_labeled_effect(runtime, &effect, "effect: headless fallback")?;
                                    Self::respond(runtime, *id, response)?;
                                } else {
                                    self.log_effect(runtime, &effect, false)?;
                                    Self::respond(runtime, *id, Response::Failed(format!("CLI frontend cannot handle {sensor:?}")))?;
                                }
                            } else {
                                self.log_effect(runtime, &effect, false)?;
                                Self::respond(runtime, *id, Response::Failed(format!("CLI frontend cannot handle {sensor:?}")))?;
                            }
                        },
                        Effect::Request { id, request: Request::Position { target }, .. } if self.headless && target == "_mouse_" => {
                            self.headless_fallbacks += 1;
                            self.log_labeled_effect(runtime, &effect, "effect: headless fallback")?;
                            Self::respond(runtime, *id, Response::Position { x: 0.0, y: 0.0 })?;
                        },
                        Effect::Bubble { text, .. } => {
                            self.log_effect(runtime, &effect, true)?;
                            if let Some(text) = text {
                                let mut output = self.output.lock().map_err(|_| io::Error::other("console output lock poisoned"))?;
                                writeln!(output, "{text}")?;
                                output.flush()?;
                            }
                        },
                    }
                    #[cfg(feature = "scratch-addons-debugger")] {
                        Effect::Debugger { target_id, thread_id, event } => {
                            match event {
                                crate::DebuggerEvent::Log { level, message } => {
                                    writeln!(self.log, "[debugger:{level}] {message} (target={target_id}, thread={thread_id})")?;
                                },
                                _ => { self.log_effect(runtime, &effect, true)?; },
                            }
                        },
                        Effect::Request { id, request: Request::DebuggerPause, .. } => {
                            self.log_effect(runtime, &effect, true)?;
                            if runtime.pending_request(*id).is_some() {
                                self.ask(*id, "Debugger breakpoint — press Enter to continue:".to_string(), InputKind::DebuggerResume)?;
                            }
                        },
                    }
                    Effect::Warning { target_id, thread_id, message } => {
                        if !runtime.config.quiet_logs {
                            writeln!(self.log, "[warning] {message} (target={target_id}, thread={thread_id})")?;
                        }
                    },
                    Effect::Request { id, request, .. } => {
                        self.log_effect(runtime, &effect, false)?;
                        Self::respond(runtime, *id, Response::Failed(format!("CLI frontend cannot handle {request:?}")))?;
                    },
                    Effect::CancelRequest { id } => {
                        self.log_effect(runtime, &effect, true)?;
                        #[cfg(feature = "scratch")]
                        if let Some(input) = self.questions.remove(id) {
                            input.active.store(false, Ordering::Release);
                        }
                        #[cfg(not(feature = "scratch"))]
                        let _ = id;
                    },
                    #[cfg(feature = "scratch")] {
                        _ => { self.log_effect(runtime, &effect, false)?; },
                    }
                }
            }
        }
        self.log.flush()?;
        #[cfg(feature = "scratch")]
        self.poll_answers(runtime)?;
        Ok(())
    }

    fn log_effect(
        &mut self,
        runtime: &Runtime,
        effect: &Effect,
        supported: bool,
    ) -> io::Result<()> {
        let label = if supported {
            "effect"
        } else {
            "effect: unsupported"
        };
        self.log_labeled_effect(runtime, effect, label)
    }

    fn log_labeled_effect(
        &mut self,
        runtime: &Runtime,
        effect: &Effect,
        label: &str,
    ) -> io::Result<()> {
        // Filter only presentation, never dispatch: quiet requests must still
        // receive responses and quiet cancellations must still cancel input.
        let enabled = cfg_variants! {
            match (effect) {
                #[cfg(feature = "scratch-addons-debugger")] {
                    Effect::Debugger { .. }
                        | Effect::Request { request: Request::DebuggerPause, .. } => true,
                }
                _ => !runtime.config.quiet_logs,
            }
        };
        if enabled {
            writeln!(self.log, "[{label}] {effect:?}")?;
        }
        Ok(())
    }

    fn respond(
        runtime: &mut Runtime,
        id: crate::RequestId,
        response: Response,
    ) -> Result<(), RuntimeError> {
        match runtime.respond(id, response) {
            // Input can finish after a cancellation or an external response.
            Err(RuntimeError::UnknownRequest(_) | RuntimeError::AlreadyResponded(_)) => Ok(()),
            result => result,
        }
    }

    #[cfg(feature = "scratch")]
    fn bounce_response(x: f64, y: f64, direction: f64) -> Response {
        if !x.is_finite() || !y.is_finite() || !direction.is_finite() {
            return Response::Failed(
                "bounce requires finite coordinates and direction".to_string(),
            );
        }
        let normalize = |angle: f64| {
            let angle = (angle + 180.0).rem_euclid(360.0) - 180.0;
            if angle == -180.0 { 180.0 } else { angle }
        };
        let mut direction = normalize(direction);
        // Scratch directions: 0 = up, 90 = right. Only reflect components
        // pointing outwards, so another bounce at the same edge won't undo it.
        if (x <= -240.0 && direction < 0.0 && direction > -180.0)
            || (x >= 240.0 && direction > 0.0 && direction < 180.0)
        {
            direction = -direction;
        }
        if (y <= -180.0 && direction.abs() > 90.0) || (y >= 180.0 && direction.abs() < 90.0) {
            direction = 180.0 - direction;
        }
        Response::Motion {
            x: x.clamp(-240.0, 240.0),
            y: y.clamp(-180.0, 180.0),
            direction: normalize(direction),
        }
    }

    #[cfg(feature = "scratch")]
    fn clock_sensor(sensor: &crate::Sensor) -> Option<Response> {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        let value = match sensor {
            crate::Sensor::Current(component) => {
                crate::Value::Number(match component.to_ascii_lowercase().as_str() {
                    "year" => now.year() as f64,
                    "month" => now.month() as f64,
                    "date" => now.day() as f64,
                    "dayofweek" => now.weekday().number_from_sunday() as f64,
                    "hour" => now.hour() as f64,
                    "minute" => now.minute() as f64,
                    "second" => now.second() as f64,
                    _ => 0.0,
                })
            }
            crate::Sensor::DaysSince2000 => {
                let epoch = chrono::NaiveDate::from_ymd_opt(2000, 1, 1)?.and_hms_opt(0, 0, 0)?;
                crate::Value::Number(
                    now.naive_local()
                        .signed_duration_since(epoch)
                        .num_milliseconds() as f64
                        / 86_400_000.0,
                )
            }
            _ => return None,
        };
        Some(Response::Value(value))
    }

    #[cfg(feature = "scratch")]
    fn headless_sensor(sensor: &crate::Sensor) -> Option<Response> {
        use crate::{Sensor, Value};
        Some(Response::Value(match sensor {
            // No rendered pixels, pressed keys, mouse button, or microphone.
            Sensor::TouchingObject(_)
            | Sensor::TouchingColor(_)
            | Sensor::ColorIsTouchingColor { .. }
            | Sensor::KeyPressed(_)
            | Sensor::MouseDown => Value::Boolean(false),
            Sensor::MouseX | Sensor::MouseY => Value::Number(0.0),
            Sensor::Loudness => Value::Number(-1.0),
            Sensor::Username => Value::String(String::new()),
            _ => return None,
        }))
    }

    #[cfg(feature = "scratch")]
    fn ask(&mut self, id: RequestId, question: String, kind: InputKind) -> io::Result<()> {
        if self.worker.is_none() {
            let input = self
                .input
                .take()
                .ok_or_else(|| io::Error::other("console input unavailable"))?;
            self.worker = Some(InputWorker::start(input, self.output.clone())?);
        }
        let active = Arc::new(AtomicBool::new(true));
        self.questions.insert(
            id,
            PendingInput {
                active: active.clone(),
                kind,
            },
        );
        self.worker
            .as_ref()
            .ok_or_else(|| io::Error::other("console input worker unavailable"))?
            .send
            .send(Question {
                id,
                text: question,
                active,
            })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "console input worker stopped"))
    }

    #[cfg(feature = "scratch")]
    fn poll_answers(&mut self, runtime: &mut Runtime) -> Result<(), CliFrontendError> {
        let Some(worker) = &self.worker else {
            return Ok(());
        };
        while let Ok((id, answer)) = worker.receive.try_recv() {
            let Some(input) = self.questions.remove(&id) else {
                continue;
            };
            if !input.active.swap(false, Ordering::AcqRel) {
                continue;
            }
            let response = match answer {
                Ok(answer) => cfg_variants! {
                    match (input.kind) {
                        InputKind::Answer => Response::Answer(answer),
                        #[cfg(feature = "scratch-addons-debugger")] {
                            InputKind::DebuggerResume => Response::Completed,
                        }
                    }
                },
                Err(error) => Response::Failed(error.to_string()),
            };
            Self::respond(runtime, id, response)?;
        }
        Ok(())
    }

    #[cfg(feature = "scratch")]
    fn cancel_questions(&mut self) {
        for (_, input) in self.questions.drain() {
            input.active.store(false, Ordering::Release);
        }
    }
}

#[cfg(feature = "scratch")]
cfg_variants! {
    enum InputKind {
        Answer,
        #[cfg(feature = "scratch-addons-debugger")] {
            DebuggerResume,
        }
    }
}

#[cfg(feature = "scratch")]
struct PendingInput {
    active: Arc<AtomicBool>,
    kind: InputKind,
}

impl Drop for CliFrontend {
    fn drop(&mut self) {
        #[cfg(feature = "scratch")]
        self.cancel_questions();
    }
}

#[cfg(feature = "scratch")]
struct Question {
    id: RequestId,
    text: String,
    active: Arc<AtomicBool>,
}

#[cfg(feature = "scratch")]
struct InputWorker {
    send: mpsc::Sender<Question>,
    receive: mpsc::Receiver<(RequestId, io::Result<String>)>,
}

#[cfg(feature = "scratch")]
impl InputWorker {
    fn start(mut input: Box<dyn BufRead + Send>, output: Output) -> io::Result<Self> {
        let (send, questions) = mpsc::channel::<Question>();
        let (answers, receive) = mpsc::channel();
        std::thread::Builder::new()
            .name("scratch-cli-input".to_string())
            .spawn(move || {
                for question in questions {
                    if !question.active.load(Ordering::Acquire) {
                        continue;
                    }
                    let answer = (|| {
                        {
                            let mut output = output
                                .lock()
                                .map_err(|_| io::Error::other("console output lock poisoned"))?;
                            write!(output, "{} ", question.text)?;
                            output.flush()?;
                        }
                        let mut answer = String::new();
                        if input.read_line(&mut answer)? == 0 {
                            return Err(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "stdin closed",
                            ));
                        }
                        Ok(answer.trim_end_matches(['\r', '\n']).to_string())
                    })();
                    if answers.send((question.id, answer)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self { send, receive })
    }
}

#[cfg(test)]
mod tests;
