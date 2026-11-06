//! Grok Voice simultaneous interpretation (`xaiSpeechMode:"voice"`, providers/xai.md
//! “Realtime”).
//!
//! The session is configured for 24 kHz PCM input with server-side turn detection and
//! instructions to interpret into the target language. The input transcription is the
//! original stream and the transcript of the spoken answer is the translation stream;
//! both are id-less and aligned by the lane (captions.md §5). Output audio is ignored.

use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use sheng_core::align::Stream;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use super::language::language_name;
use super::protocol::{classify_error, parse, separator, text_field, Inbound, Protocol};
use crate::provider::{ProviderError, ProviderEvent, StreamText};

pub const VOICE_MODEL: &str = "grok-voice-latest";
/// The last turn is awaited at most this long after input ends.
pub const VOICE_FINISH_WAIT: Duration = Duration::from_secs(2);
/// `response.done` status that fails the session (OpenAI-compatible; `cancelled` and
/// `incomplete` are interrupted turns, not failures).
const FAILED_STATUS: &str = "failed";

/// Adds the model to the realtime endpoint.
pub fn voice_url(endpoint: &Url) -> Url {
    let mut url = endpoint.clone();
    url.query_pairs_mut().append_pair("model", VOICE_MODEL);
    url
}

fn instructions(target: &str) -> String {
    let language = language_name(target);
    format!(
        "You are a simultaneous interpreter. Translate everything the speaker says into \
         {language}. Speak only the translation: do not answer questions, add comments or \
         repeat the original."
    )
}

/// The `session.update` sent after connecting.
pub fn session_update(sample_rate: u32, source: &str, target: &str) -> Value {
    let mut input = json!({"format": {"type": "audio/pcm", "rate": sample_rate}});
    if source != sheng_core::language::AUTO {
        input["transcription"] = json!({"language_hint": source});
    }
    json!({
        "type": "session.update",
        "session": {
            "instructions": instructions(target),
            "turn_detection": {"type": "server_vad"},
            "audio": {
                "input": input,
                "output": {"format": {"type": "audio/pcm", "rate": sample_rate}},
            },
        },
    })
}

#[derive(Debug)]
pub struct VoiceProtocol {
    setup: String,
    /// Last text sent on each stream, to separate the next piece from it.
    last_original: String,
    last_translation: String,
    /// A translation delta of the current response was already sent.
    in_response: bool,
}

impl VoiceProtocol {
    pub fn new(sample_rate: u32, source: &str, target: &str) -> Self {
        Self {
            setup: session_update(sample_rate, source, target).to_string(),
            last_original: String::new(),
            last_translation: String::new(),
            in_response: false,
        }
    }

    /// A new piece of `stream`; `continues` when it extends the previous piece directly.
    fn piece(stream: Stream, last: &mut String, text: &str, continues: bool) -> Vec<ProviderEvent> {
        if text.is_empty() {
            return Vec::new();
        }
        let gap = if continues { "" } else { separator(last, text) };
        let delta = format!("{gap}{text}");
        *last = text.to_owned();
        vec![ProviderEvent::StreamText {
            stream,
            text: StreamText::Delta(delta),
        }]
    }

    fn on_response_done(message: &Value) -> Inbound {
        let status = message
            .get("response")
            .and_then(|response| response.get("status"))
            .and_then(Value::as_str);
        if status == Some(FAILED_STATUS) {
            return Inbound::failure(ProviderError::Provider);
        }
        Inbound {
            events: vec![ProviderEvent::TurnComplete],
            turn_end: true,
            ..Inbound::default()
        }
    }
}

impl Protocol for VoiceProtocol {
    fn setup(&self) -> Vec<String> {
        vec![self.setup.clone()]
    }

    fn audio(pcm: &[u8]) -> Message {
        let audio = base64::engine::general_purpose::STANDARD.encode(pcm);
        let message = json!({"type": "input_audio_buffer.append", "audio": audio});
        Message::Text(message.to_string().into())
    }

    fn finish(&self) -> Option<String> {
        None
    }

    fn finish_wait(&self) -> Duration {
        VOICE_FINISH_WAIT
    }

    fn on_text(&mut self, text: &str) -> Inbound {
        let Some((message, kind)) = parse(text) else {
            return Inbound::failure(ProviderError::Provider);
        };
        match kind.as_str() {
            "session.updated" => Inbound {
                ready: true,
                ..Inbound::default()
            },
            "conversation.item.input_audio_transcription.completed" => {
                let text = text_field(&message, "transcript");
                let last = &mut self.last_original;
                Inbound::events(Self::piece(Stream::Original, last, text, false))
            }
            "response.output_audio_transcript.delta" => {
                let text = text_field(&message, "delta");
                let last = &mut self.last_translation;
                let events = Self::piece(Stream::Translation, last, text, self.in_response);
                self.in_response |= !events.is_empty();
                Inbound::events(events)
            }
            "response.done" => {
                self.in_response = false;
                Self::on_response_done(&message)
            }
            "error" => Inbound::failure(classify_error(&message)),
            _ => Inbound::default(),
        }
    }

    fn flush(&mut self) -> Vec<ProviderEvent> {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "voice_tests.rs"]
mod tests;
