//! Live-translation client over the realtime WebSocket (`alibabaRealtimeMode:"liveTranslate"`).
//! Opened by request validation on 2026-11-05 (start-request.md “路由”).

use url::Url;

use super::protocol::{self, LiveEvent};
use super::state::LiveState;
use crate::config::endpoint::ProxyChoice;
use crate::config::secret::Secret;
use crate::provider::{BoxFuture, EventSink, ProviderError, ProviderEvent, SpeechClient};
use crate::services::alibaba::asr::client::bearer;
use crate::services::alibaba::connection::{self, Link, Reader};
use crate::services::alibaba::diagnostics::{FailureDetail, FailureNote};
use crate::services::alibaba::errors;
use crate::transport::WsRequest;
use tokio_tungstenite::tungstenite::Message;

/// Connection settings. `Debug` hides the key.
#[derive(Debug, Clone)]
pub struct LiveConfig {
    pub endpoint: Url,
    pub api_key: Secret,
    pub proxy: ProxyChoice,
    pub target_language: String,
    pub sample_rate: u32,
}

pub struct LiveTranslateClient {
    config: LiveConfig,
    link: Link,
    note: FailureNote,
}

impl LiveTranslateClient {
    pub fn new(config: LiveConfig) -> Self {
        Self {
            config,
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
        let request = WsRequest {
            url: self.config.endpoint.clone(),
            headers: vec![("authorization", bearer(&self.config.api_key))],
        };
        let (mut writer, mut reader) = connection::open(&request, &self.config.proxy).await?;
        wait_for(&mut reader, &self.note, &LiveEvent::SessionCreated).await?;
        let update =
            protocol::session_update(&self.config.target_language, self.config.sample_rate);
        connection::send_json(&mut writer, &update).await?;
        wait_for(&mut reader, &self.note, &LiveEvent::SessionUpdated).await?;
        events
            .emit(ProviderEvent::Ready)
            .map_err(|_| ProviderError::Transport)?;
        self.link = Link::spawn(writer, reader, LiveState::default(), events, &self.note);
        Ok(())
    }
}

/// Handshake step: skips unrelated events, fails on `error`.
async fn wait_for(
    reader: &mut Reader,
    note: &FailureNote,
    expected: &LiveEvent,
) -> Result<(), ProviderError> {
    loop {
        let text = connection::next_text(reader, note).await?;
        match protocol::parse(&text) {
            event if &event == expected => return Ok(()),
            LiveEvent::Error { code, message } => {
                return Err(errors::classify(code.as_deref(), message.as_deref()))
            }
            LiveEvent::Finished => return Err(ProviderError::Transport),
            _ => {}
        }
    }
}

impl SpeechClient for LiveTranslateClient {
    fn connect(&mut self, events: EventSink) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.open(events))
    }

    fn send_audio(&mut self, pcm: Vec<u8>) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            let append = protocol::audio_append(&pcm);
            connection::send_json(self.link.writer()?, &append).await
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
            connection::send_json(self.link.writer()?, &protocol::session_finish()).await
        })
    }

    fn disconnect(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(self.link.close())
    }
}
