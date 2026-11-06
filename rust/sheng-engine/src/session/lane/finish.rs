//! Stop sequence of a lane once audio input is closed: drain the send queue, end input,
//! wait for the last results, then let the pipeline drain (snapshot.md, pipeline §3).

use std::time::Duration;

use sheng_runtime::audio::drain_within;
use sheng_runtime::audio::lane::SEND_TIMEOUT;
use sheng_runtime::config::route::SpeechRoute;
use sheng_runtime::pipeline::PipelineInput;
use sheng_runtime::provider::{SessionError, SpeechClient};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::{next_output, sender_result, until, Lane};
use crate::session::state::Phase;

/// Last results are awaited at most this long after input ends.
pub const MAX_FINISH_WAIT: Duration = Duration::from_secs(8);

/// How long each kind of service gets to deliver its last final (pipeline §3).
fn recognizer_wait(route: SpeechRoute) -> Duration {
    match route {
        SpeechRoute::AppleSpeech
        | SpeechRoute::AlibabaAsr
        | SpeechRoute::CustomDashScope
        | SpeechRoute::CustomOpenAi => Duration::from_secs(1),
        SpeechRoute::OpenAiTranscribe
        | SpeechRoute::OpenAiTranslate
        | SpeechRoute::AzureTranscribe
        | SpeechRoute::AzureTranslate
        | SpeechRoute::XaiStt
        | SpeechRoute::XaiVoice => Duration::from_secs(2),
        SpeechRoute::GeminiTranscribe | SpeechRoute::GeminiTranslate => Duration::from_millis(4500),
        SpeechRoute::AlibabaLiveTranslate
        | SpeechRoute::Volcano
        | SpeechRoute::Tencent
        | SpeechRoute::Baidu => Duration::from_secs(6),
    }
}

fn aborted(phase: Phase) -> bool {
    phase == Phase::Aborted
}

impl Lane {
    pub(super) async fn drain_and_finish(
        &mut self,
        sender: Option<JoinHandle<Result<(), SessionError>>>,
    ) -> Result<(), SessionError> {
        if let Some(sender) = sender {
            let abort = sender.abort_handle();
            let drained = drain_within(async { sender_result(sender.await) }).await;
            if drained.is_err() {
                abort.abort();
            }
            drained?;
        }
        let deadline = Instant::now() + MAX_FINISH_WAIT;
        match tokio::time::timeout(SEND_TIMEOUT, self.client.finish()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error.speech_session_error()),
            Err(_) => return Err(SessionError::TransportError),
        }
        let recognizer = (Instant::now() + recognizer_wait(self.ctx.plan.route)).min(deadline);
        if !self.pump_until(recognizer, |lane| lane.ended).await? {
            return Ok(());
        }
        self.flush_streams()?;
        let Some(pipeline) = &mut self.pipeline else {
            return self.ctx.commit_unconfirmed(self.source);
        };
        // A draft the recognizer never finalized is translated like a final (README
        // “结束时不丢尾句”).
        if let Some((utterance, text)) = pipeline.shown_draft.take() {
            if !pipeline.send_final(utterance, text).await {
                return Ok(());
            }
        }
        if !pipeline.send_in_order(PipelineInput::Finish).await {
            return Ok(());
        }
        let in_time = self
            .pump_until(deadline, |lane| {
                lane.pipeline.as_ref().is_none_or(|p| p.finished)
            })
            .await?;
        if in_time {
            self.ctx.commit_unconfirmed(self.source)?;
        }
        Ok(())
    }

    /// Handles events and pipeline results until `done` or `deadline`. Returns false when
    /// the session was aborted meanwhile.
    async fn pump_until(
        &mut self,
        deadline: Instant,
        done: fn(&Lane) -> bool,
    ) -> Result<bool, SessionError> {
        while !done(self) {
            tokio::select! {
                event = self.events.recv() => {
                    if event.is_none() {
                        return Ok(true);
                    }
                    self.on_event(event).await?;
                }
                output = next_output(&mut self.pipeline) => self.on_output(output)?,
                _ = tokio::time::sleep_until(deadline) => return Ok(true),
                _ = until(&mut self.phase, aborted) => return Ok(false),
            }
        }
        Ok(true)
    }
}
