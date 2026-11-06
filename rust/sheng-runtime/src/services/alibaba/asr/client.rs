//! Streaming recognition client over the inference WebSocket (alibabaCloud asrMt and
//! customDashScopeASR).

use sheng_core::{CaptionEvent, UtteranceId};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use super::protocol::{self, ServerEvent, TaskSettings};
use crate::config::endpoint::ProxyChoice;
use crate::config::secret::Secret;
use crate::provider::{BoxFuture, EventSink, ProviderError, ProviderEvent, SpeechClient};
use crate::services::alibaba::connection::{self, Inbound, Link, Reader, Step};
use crate::services::alibaba::diagnostics::{FailureDetail, FailureNote};
use crate::transport::WsRequest;

/// Everything needed to open one recognition task. `Debug` hides the key.
#[derive(Debug, Clone)]
pub struct AsrConfig {
    pub endpoint: Url,
    pub api_key: Secret,
    pub proxy: ProxyChoice,
    pub settings: TaskSettings,
}

pub fn bearer(api_key: &Secret) -> Secret {
    Secret::new(format!("Bearer {}", api_key.expose()))
}

pub struct AsrClient {
    config: AsrConfig,
    task_id: String,
    link: Link,
    note: FailureNote,
}

impl AsrClient {
    pub fn new(config: AsrConfig) -> Self {
        Self {
            config,
            task_id: String::new(),
            link: Link::default(),
            note: FailureNote::default(),
        }
    }

    /// Close frame and error fields the service reported on the current connection.
    pub fn failure_detail(&self) -> FailureDetail {
        self.note.detail()
    }

    async fn open(&mut self, events: EventSink) -> Result<(), ProviderError> {
        self.link.close().await;
        self.note.clear();
        self.task_id = uuid::Uuid::new_v4().to_string();
        let request = WsRequest {
            url: self.config.endpoint.clone(),
            headers: vec![("authorization", bearer(&self.config.api_key))],
        };
        let (mut writer, mut reader) = connection::open(&request, &self.config.proxy).await?;
        let run_task = protocol::run_task(&self.task_id, &self.config.settings);
        connection::send_json(&mut writer, &run_task).await?;
        let early = wait_started(&mut reader, &self.task_id, &self.note).await?;
        events
            .emit(ProviderEvent::Ready)
            .map_err(|_| ProviderError::Transport)?;
        for event in early {
            events.emit(event).map_err(|_| ProviderError::Transport)?;
        }
        let handler = AsrInbound {
            task_id: self.task_id.clone(),
            draft: None,
        };
        self.link = Link::spawn(writer, reader, handler, events, &self.note);
        Ok(())
    }
}

/// Waits for `task-started`; audio is only sent after it. Results that arrive before it
/// (not expected) are kept and reported after `Ready`.
async fn wait_started(
    reader: &mut Reader,
    task_id: &str,
    note: &FailureNote,
) -> Result<Vec<ProviderEvent>, ProviderError> {
    let mut early = Vec::new();
    loop {
        let text = connection::next_text(reader, note).await?;
        match protocol::parse(&text, task_id) {
            ServerEvent::Started => return Ok(early),
            ServerEvent::Failed(error) => return Err(error),
            ServerEvent::Finished => return Err(ProviderError::Transport),
            ServerEvent::Caption(event) => early.push(ProviderEvent::Caption(event)),
            ServerEvent::Ignored => {}
        }
    }
}

struct AsrInbound {
    task_id: String,
    /// Latest draft of the sentence in progress, until its final.
    draft: Option<(Option<UtteranceId>, String)>,
}

impl Inbound for AsrInbound {
    fn on_text(&mut self, text: &str) -> Step {
        match protocol::parse(text, &self.task_id) {
            ServerEvent::Caption(event) => {
                self.draft = match &event {
                    CaptionEvent::OriginalDraft { utterance, text } => {
                        Some((utterance.clone(), text.clone()))
                    }
                    _ => None,
                };
                Step::events(vec![ProviderEvent::Caption(event)])
            }
            ServerEvent::Finished => Step::last(ProviderEvent::Ended),
            ServerEvent::Failed(error) => Step::last(ProviderEvent::Failed(error)),
            ServerEvent::Started | ServerEvent::Ignored => Step::default(),
        }
    }

    fn interrupted(&mut self) -> Vec<ProviderEvent> {
        self.draft
            .take()
            .map(|(utterance, text)| {
                ProviderEvent::Caption(CaptionEvent::OriginalFinal { utterance, text })
            })
            .into_iter()
            .collect()
    }
}

impl SpeechClient for AsrClient {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.open(events))
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            let writer = self.link.writer()?;
            connection::send(writer, Message::Binary(pcm.into())).await
        })
    }

    fn heartbeat(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            let writer = self.link.writer()?;
            connection::send(writer, Message::Ping(Vec::new().into())).await
        })
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            self.link.mark_finishing();
            let finish = protocol::finish_task(&self.task_id);
            let writer = self.link.writer()?;
            connection::send_json(writer, &finish).await
        })
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(self.link.close())
    }
}
