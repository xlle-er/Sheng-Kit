//! Live API messages (ai.google.dev Live API reference, Live Translate and Live Transcribe
//! guides): the client messages this engine sends and the server fields it reads.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::errors::classify_status;
use crate::provider::ProviderError;

/// Input audio format for both Gemini routes.
pub const AUDIO_MIME_TYPE: &str = "audio/pcm;rate=16000";

/// What one session asks the service for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionKind {
    /// Speech translation: input and output transcripts, translated audio ignored.
    Translate { target: String },
    /// Transcription only, with an optional source language hint.
    Transcribe { source: Option<String> },
}

/// First client message (`BidiGenerateContentSetup`).
pub fn setup_message(model: &str, kind: &SessionKind) -> String {
    let mut setup = Map::new();
    setup.insert("model".into(), json!(format!("models/{model}")));
    match kind {
        SessionKind::Translate { target } => {
            setup.insert(
                "generationConfig".into(),
                json!({
                    "responseModalities": ["AUDIO"],
                    "translationConfig": {
                        "targetLanguageCode": target,
                        "echoTargetLanguage": true,
                    },
                }),
            );
            setup.insert("inputAudioTranscription".into(), json!({}));
            setup.insert("outputAudioTranscription".into(), json!({}));
        }
        SessionKind::Transcribe { source } => {
            setup.insert(
                "generationConfig".into(),
                json!({ "responseModalities": ["TEXT"] }),
            );
            let transcription = match source {
                Some(code) => json!({ "languageCodes": [code] }),
                None => json!({}),
            };
            setup.insert("inputAudioTranscription".into(), transcription);
        }
    }
    json!({ "setup": setup }).to_string()
}

/// One chunk of 16 kHz PCM as `realtimeInput.audio`.
pub fn audio_message(pcm: &[u8]) -> String {
    json!({
        "realtimeInput": {
            "audio": { "data": BASE64.encode(pcm), "mimeType": AUDIO_MIME_TYPE }
        }
    })
    .to_string()
}

/// Tells the service that input has ended.
pub fn audio_stream_end_message() -> String {
    json!({ "realtimeInput": { "audioStreamEnd": true } }).to_string()
}

/// Server facts the client acts on, in the order they apply within one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    SetupComplete,
    /// `inputTranscription.text`.
    Input(String),
    /// `interimInputTranscription.text`.
    InterimInput(String),
    /// `outputTranscription.text`.
    Output(String),
    Interrupted,
    GenerationComplete,
    TurnComplete,
    GoAway,
    Failed(ProviderError),
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerMessage {
    setup_complete: Option<Value>,
    server_content: Option<ServerContent>,
    go_away: Option<Value>,
    error: Option<ErrorBody>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerContent {
    input_transcription: Option<Transcription>,
    interim_input_transcription: Option<Transcription>,
    output_transcription: Option<Transcription>,
    #[serde(default)]
    interrupted: bool,
    #[serde(default)]
    generation_complete: bool,
    #[serde(default)]
    turn_complete: bool,
}

#[derive(Debug, Default, Deserialize)]
struct Transcription {
    #[serde(default)]
    text: String,
}

#[derive(Debug, Default, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    status: String,
}

fn text_event(
    transcription: Option<Transcription>,
    make: fn(String) -> ServerEvent,
) -> Option<ServerEvent> {
    transcription
        .map(|t| t.text)
        .filter(|text| !text.is_empty())
        .map(make)
}

fn content_events(content: ServerContent, events: &mut Vec<ServerEvent>) {
    events.extend(text_event(content.input_transcription, ServerEvent::Input));
    events.extend(text_event(
        content.interim_input_transcription,
        ServerEvent::InterimInput,
    ));
    events.extend(text_event(
        content.output_transcription,
        ServerEvent::Output,
    ));
    let flags = [
        (content.interrupted, ServerEvent::Interrupted),
        (content.generation_complete, ServerEvent::GenerationComplete),
        (content.turn_complete, ServerEvent::TurnComplete),
    ];
    events.extend(flags.into_iter().filter(|(set, _)| *set).map(|(_, e)| e));
}

/// Parses one server message. Unknown fields (model audio, usage, tool calls) are ignored;
/// a message that is not JSON is a provider error.
pub fn parse_server_message(raw: &[u8]) -> Vec<ServerEvent> {
    let Ok(message) = serde_json::from_slice::<ServerMessage>(raw) else {
        return vec![ServerEvent::Failed(ProviderError::Provider)];
    };
    let mut events = Vec::new();
    if let Some(error) = message.error {
        events.push(ServerEvent::Failed(classify_status(&error.status)));
        return events;
    }
    if message.setup_complete.is_some() {
        events.push(ServerEvent::SetupComplete);
    }
    if let Some(content) = message.server_content {
        content_events(content, &mut events);
    }
    if message.go_away.is_some() {
        events.push(ServerEvent::GoAway);
    }
    events
}
