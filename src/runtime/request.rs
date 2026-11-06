use crate::{Value, cfg_variants};

pub type RequestId = u64;

cfg_variants! {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Request {
        /// An application-defined request, usable without Scratch support.
        External { name: String, arguments: Vec<Value> },

        #[cfg(feature = "scratch-addons-debugger")] {
            /// Pauses the VM until Completed (or Failed) is supplied.
            DebuggerPause,
        }

        #[cfg(feature = "scratch")] {
            Ask { question: String },
            PlaySoundUntilDone { sound: String },
            Speak { words: String, voice: String, language: String },
            Translate { words: String, language: String },
            Sense(Sensor),
            Position { target: String },
            Bounce { x: f64, y: f64, direction: f64 },
        }
    }
}

cfg_variants! {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Response {
        Value(Value),
        Completed,
        Failed(String),

        #[cfg(feature = "scratch")] {
            Answer(String),
            Position { x: f64, y: f64 },
            Motion { x: f64, y: f64, direction: f64 },
        }
    }
}

#[cfg(feature = "scratch")]
#[derive(Debug, Clone, PartialEq)]
pub enum Sensor {
    TouchingObject(String),
    TouchingColor(String),
    ColorIsTouchingColor { color: String, color2: String },
    KeyPressed(String),
    MouseDown,
    MouseX,
    MouseY,
    Loudness,
    Current(String),
    DaysSince2000,
    Username,
    UserId,
    Video { attribute: String, subject: String },
    ViewerLanguage,
}

impl Request {
    pub(crate) fn accepts(&self, response: &Response) -> bool {
        if matches!(response, Response::Failed(_)) {
            return true;
        }
        crate::cfg_variants! {
            match (self) {
                Request::External { .. } => matches!(response, Response::Value(_) | Response::Completed),
                #[cfg(feature = "scratch-addons-debugger")] {
                    Request::DebuggerPause => matches!(response, Response::Completed),
                }
                #[cfg(feature = "scratch")] {
                    Request::Ask { .. } => matches!(response, Response::Answer(_)),
                    Request::PlaySoundUntilDone { .. } | Request::Speak { .. } => matches!(response, Response::Completed),
                    Request::Translate { .. } => matches!(response, Response::Value(Value::String(_))),
                    Request::Sense(_) => matches!(response, Response::Value(_)),
                    Request::Position { .. } => matches!(response, Response::Position { .. }),
                    Request::Bounce { .. } => matches!(response, Response::Motion { .. }),
                }
            }
        }
    }
}
