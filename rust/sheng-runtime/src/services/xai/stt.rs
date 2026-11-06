//! Streaming speech-to-text (`xaiSpeechMode:"asrMt"`, providers/xai.md “STT”).
//!
//! The service configures the stream from query parameters, takes raw PCM as binary frames
//! and reports `transcript.partial` results: interim (`is_final:false`), chunk final
//! (`is_final:true`, text of a locked chunk) and utterance final (`speech_final:true`,
//! the whole utterance). Utterances carry no id; they are numbered per connection.

use std::time::Duration;

use serde_json::Value;
use sheng_core::{CaptionEvent, UtteranceId};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use super::protocol::{classify_error, join, parse, text_field, Inbound, Protocol};
use crate::provider::{ProviderError, ProviderEvent};

pub const STT_MODEL: &str = "grok-voice-transcribe-2.0";
/// After `audio.done`, `transcript.done` is awaited at most this long.
pub const STT_FINISH_WAIT: Duration = Duration::from_secs(1);

/// Adds the stream configuration to the STT endpoint.
pub fn stt_url(endpoint: &Url, sample_rate: u32, source_language: &str) -> Url {
    let mut url = endpoint.clone();
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("model", STT_MODEL)
            .append_pair("encoding", "pcm")
            .append_pair("sample_rate", &sample_rate.to_string())
            .append_pair("interim_results", "true");
        if source_language != sheng_core::language::AUTO {
            query.append_pair("language", source_language);
        }
    }
    url
}

#[derive(Debug, Default)]
pub struct SttProtocol {
    /// Number of the utterance being recognized.
    utterance: u64,
    /// Locked chunks of the current utterance.
    locked: String,
    /// Latest interim text after the locked chunks.
    interim: String,
}

impl SttProtocol {
    pub fn new() -> Self {
        Self {
            utterance: 1,
            ..Self::default()
        }
    }

    fn id(&self) -> Option<UtteranceId> {
        Some(UtteranceId::Number(self.utterance))
    }

    fn current(&self) -> String {
        join(&self.locked, &self.interim)
    }

    fn draft(&self) -> Vec<ProviderEvent> {
        let text = self.current();
        if text.trim().is_empty() {
            return Vec::new();
        }
        vec![ProviderEvent::Caption(CaptionEvent::OriginalDraft {
            utterance: self.id(),
            text,
        })]
    }

    /// Confirms `text` as the current utterance and starts the next one.
    fn confirm(&mut self, text: String) -> Vec<ProviderEvent> {
        self.locked.clear();
        self.interim.clear();
        if text.trim().is_empty() {
            return Vec::new();
        }
        let event = CaptionEvent::OriginalFinal {
            utterance: self.id(),
            text,
        };
        self.utterance += 1;
        vec![ProviderEvent::Caption(event)]
    }

    fn on_partial(&mut self, message: &Value) -> Result<Vec<ProviderEvent>, ProviderError> {
        let flag = |name: &str| message.get(name).and_then(Value::as_bool);
        let (Some(is_final), text) = (flag("is_final"), text_field(message, "text")) else {
            return Err(ProviderError::Provider);
        };
        let speech_final = flag("speech_final").unwrap_or(false);
        if is_final && speech_final {
            let whole = if text.trim().is_empty() {
                self.current()
            } else {
                text.to_owned()
            };
            return Ok(self.confirm(whole));
        }
        if is_final {
            self.locked = join(&self.locked, text);
            self.interim.clear();
        } else {
            self.interim = text.to_owned();
        }
        Ok(self.draft())
    }
}

impl Protocol for SttProtocol {
    fn setup(&self) -> Vec<String> {
        Vec::new()
    }

    fn audio(pcm: &[u8]) -> Message {
        Message::Binary(pcm.to_vec().into())
    }

    fn finish(&self) -> Option<String> {
        Some(r#"{"type":"audio.done"}"#.to_owned())
    }

    fn finish_wait(&self) -> Duration {
        STT_FINISH_WAIT
    }

    fn on_text(&mut self, text: &str) -> Inbound {
        let Some((message, kind)) = parse(text) else {
            return Inbound::failure(ProviderError::Provider);
        };
        match kind.as_str() {
            "transcript.created" => Inbound {
                ready: true,
                ..Inbound::default()
            },
            "transcript.partial" => match self.on_partial(&message) {
                Ok(events) => Inbound::events(events),
                Err(error) => Inbound::failure(error),
            },
            // The text of `transcript.done` covers the whole stream; finals were already
            // reported per utterance, so only an unconfirmed remainder is confirmed.
            "transcript.done" => Inbound {
                events: self.flush(),
                done: true,
                ..Inbound::default()
            },
            "error" => Inbound::failure(classify_error(&message)),
            _ => Inbound::default(),
        }
    }

    fn flush(&mut self) -> Vec<ProviderEvent> {
        let rest = self.current();
        self.confirm(rest)
    }
}

#[cfg(test)]
#[path = "stt_tests.rs"]
mod tests;
