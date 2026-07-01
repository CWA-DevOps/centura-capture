// audio/transcription/deepgram.rs
//
// Centura Capture (M2): Deepgram live streaming STT backend.
//
// Opens one WebSocket per meeting to Deepgram's streaming API WITHOUT diarization
// (diarize=false — plain transcript; speaker labels were unreliable) and no-retention
// (mip_opt_out=true). Audio frames come from the existing VAD AudioChunk receiver
// (16 kHz mono f32), are converted to linear16, and streamed up. Final results are
// parsed and emitted as the same `transcript-update` event the local engines use.
//
// Accuracy: firm-specific vocabulary can be biased via keyterm prompting —
// DEEPGRAM_KEYTERMS in .env (comma-separated, ≤500 tokens total; ~20–50 terms is
// the sweet spot). Free on nova-3.
//
// Robustness: if the WebSocket drops MID-meeting, we reconnect with exponential
// backoff (up to RECONNECT_BUDGET). Audio keeps buffering in the unbounded channel
// during the outage, so a successful reconnect loses nothing. If reconnection is
// exhausted we emit a structured `transcription-error` (the UI stops the recording
// cleanly and the partial transcript is saved + vault-exported).
//
// Fallback: if Deepgram is not the selected provider, no key is present, or the
// FIRST connect fails, `maybe_run_deepgram` hands the receiver back so the caller
// can run the local engine instead. (No mid-session engine switch: the local model
// may not be installed, and mixed engines would interleave sequence ids.)

use super::worker::TranscriptUpdate;
use crate::audio::AudioChunk;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use serde::Deserialize;
use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_MODEL: &str = "nova-3";
const LANGUAGE: &str = "en-US";
/// Total time budget for mid-session reconnect attempts before giving up.
const RECONNECT_BUDGET: Duration = Duration::from_secs(120);
/// Max wait for Deepgram to deliver final results + close after CloseStream,
/// so a hung server can't stall stop_recording.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

// Sequence counter for Deepgram-emitted transcript updates (separate from the
// local worker's counter; the two paths never run simultaneously). Never reset:
// the frontend dedups on sequence_id, so ids must stay unique across meetings
// within an app run.
static SEQUENCE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Outcome of the Deepgram branch in `start_transcription_task`.
pub enum DeepgramOutcome {
    /// Deepgram ran the whole session and consumed the receiver.
    Handled,
    /// Deepgram is not used (not selected / no key / first connect failed); the
    /// caller should run the local engine with the returned receiver.
    FallBack(UnboundedReceiver<AudioChunk>),
}

/// Read the Deepgram API key from the environment (loaded from .env at startup).
/// Returns None for an absent or empty value.
pub fn api_key() -> Option<String> {
    match std::env::var("DEEPGRAM_API_KEY") {
        Ok(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
        _ => None,
    }
}

/// Key terms from DEEPGRAM_KEYTERMS (.env, comma-separated). Biases nova-3 toward
/// firm vocabulary (names, vendors, jargon) at no extra cost. Edit .env and restart
/// the app to reload — same lifecycle as the API key.
fn keyterms() -> Vec<String> {
    match std::env::var("DEEPGRAM_KEYTERMS") {
        Ok(v) => v
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Percent-encode a query-string value (RFC 3986 unreserved chars pass through).
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// If the configured provider is Deepgram and a key is available, connect and run
/// the streaming session (reconnecting on mid-session drops). Otherwise (or on
/// first-connect failure) return the receiver for local fallback.
pub async fn maybe_run_deepgram<R: Runtime>(
    app: AppHandle<R>,
    receiver: UnboundedReceiver<AudioChunk>,
) -> DeepgramOutcome {
    // Which provider is configured?
    let (provider, model) = match crate::api::api::api_get_transcript_config(
        app.clone(),
        app.clone().state(),
        None,
    )
    .await
    {
        Ok(Some(cfg)) => (cfg.provider, cfg.model),
        _ => (String::new(), String::new()),
    };

    if provider != "deepgram" {
        return DeepgramOutcome::FallBack(receiver);
    }

    let key = match api_key() {
        Some(k) => k,
        None => {
            warn!("Deepgram selected but DEEPGRAM_API_KEY is not set; falling back to local STT");
            let _ = app.emit(
                "transcription-warning",
                "Deepgram selected but no API key found in .env — using local transcription.",
            );
            return DeepgramOutcome::FallBack(receiver);
        }
    };

    let model = if model.trim().is_empty() {
        DEFAULT_MODEL.to_string()
    } else {
        model
    };

    info!(
        "🌐 Deepgram: connecting (model='{}', language='{}', diarize=false, mip_opt_out=true)",
        model, LANGUAGE
    );

    // First connect: failure here falls back to the local engine, which can still
    // handle the whole meeting.
    let mut ws = match connect(&key, &model).await {
        Ok(ws) => ws,
        Err(e) => {
            error!("❌ Deepgram connect failed: {} — falling back to local STT", e);
            let _ = app.emit(
                "transcription-warning",
                format!("Deepgram unavailable ({}). Using local transcription.", e),
            );
            return DeepgramOutcome::FallBack(receiver);
        }
    };
    info!("✅ Deepgram WebSocket connected");

    // Session loop: run until the recording stops, reconnecting on WS failure.
    let mut receiver = receiver;
    let mut pending: Option<AudioChunk> = None;
    loop {
        match run_session(&app, receiver, pending.take(), ws).await {
            SessionEnd::Completed => return DeepgramOutcome::Handled,
            SessionEnd::Failed {
                receiver: r,
                pending: p,
            } => {
                receiver = r;
                pending = p;
                match reconnect(&key, &model).await {
                    Some(new_ws) => {
                        ws = new_ws;
                        // Queued chunks (buffered in the channel during the outage)
                        // stream up on the fresh connection — nothing lost.
                    }
                    None => {
                        error!(
                            "❌ Deepgram: reconnect budget ({:?}) exhausted — stopping transcription",
                            RECONNECT_BUDGET
                        );
                        let _ = app.emit(
                            "transcription-error",
                            serde_json::json!({
                                "error": "deepgram_connection_lost",
                                "userMessage": "Lost connection to the transcription service and couldn't reconnect. Recording stopped — the transcript up to this point was saved.",
                                "actionable": false
                            }),
                        );
                        // Drain and discard queued audio so the pipeline's sender
                        // doesn't back up; recv() ends when recording stops.
                        let mut dropped: usize = if pending.is_some() { 1 } else { 0 };
                        while receiver.recv().await.is_some() {
                            dropped += 1;
                        }
                        warn!(
                            "Deepgram: discarded {} audio chunk(s) after connection loss",
                            dropped
                        );
                        return DeepgramOutcome::Handled;
                    }
                }
            }
        }
    }
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Open the Deepgram streaming WebSocket with the Authorization header.
async fn connect(key: &str, model: &str) -> Result<WsStream, String> {
    let mut url = format!(
        "wss://api.deepgram.com/v1/listen\
?model={model}\
&language={LANGUAGE}\
&encoding=linear16\
&sample_rate=16000\
&channels=1\
&punctuate=true\
&smart_format=true\
&diarize=false\
&interim_results=false\
&mip_opt_out=true"
    );

    // Keyterm prompting (nova-3): bias recognition toward firm vocabulary. Free.
    let terms = keyterms();
    if !terms.is_empty() {
        info!(
            "Deepgram: applying {} keyterm(s) from DEEPGRAM_KEYTERMS",
            terms.len()
        );
        for t in &terms {
            url.push_str("&keyterm=");
            url.push_str(&url_encode(t));
        }
    }

    let mut request = url
        .into_client_request()
        .map_err(|e| format!("bad request: {}", e))?;
    let auth = HeaderValue::from_str(&format!("Token {}", key))
        .map_err(|e| format!("bad auth header: {}", e))?;
    request.headers_mut().insert("Authorization", auth);

    let (ws, _resp) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("websocket connect error: {}", e))?;
    Ok(ws)
}

/// Try to re-establish the WebSocket with exponential backoff (1s → 15s cap),
/// giving up once RECONNECT_BUDGET is spent.
async fn reconnect(key: &str, model: &str) -> Option<WsStream> {
    let started = Instant::now();
    let mut delay = Duration::from_secs(1);
    let mut attempt: u32 = 1;
    loop {
        warn!("Deepgram: reconnect attempt {}…", attempt);
        match connect(key, model).await {
            Ok(ws) => {
                info!("✅ Deepgram reconnected after {} attempt(s)", attempt);
                return Some(ws);
            }
            Err(e) => warn!("Deepgram: reconnect attempt {} failed: {}", attempt, e),
        }
        if started.elapsed() + delay > RECONNECT_BUDGET {
            return None;
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(15));
        attempt += 1;
    }
}

/// How a streaming session ended.
enum SessionEnd {
    /// Recording stopped; CloseStream sent and final results drained.
    Completed,
    /// The WebSocket failed while recording was still live. The caller gets the
    /// receiver back (audio keeps buffering in the channel) plus any chunk that
    /// was in flight when the send failed, so it can reconnect and resume.
    Failed {
        receiver: UnboundedReceiver<AudioChunk>,
        pending: Option<AudioChunk>,
    },
}

/// Run one streaming session on an open WebSocket: pump audio up, emit transcript
/// results down, keepalives during silence. A single select loop owns both halves
/// so the receiver can be handed back intact on failure.
async fn run_session<R: Runtime>(
    app: &AppHandle<R>,
    mut receiver: UnboundedReceiver<AudioChunk>,
    mut pending: Option<AudioChunk>,
    ws: WsStream,
) -> SessionEnd {
    let (mut write, mut read) = ws.split();
    let mut keepalive = tokio::time::interval(Duration::from_secs(5));
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut speech_emitted = false;

    // Re-send the chunk that was in flight when the previous session failed.
    if let Some(chunk) = pending.take() {
        let pcm = to_linear16(&chunk);
        if write.send(Message::binary(pcm)).await.is_err() {
            warn!("Deepgram: resend of pending chunk failed");
            return SessionEnd::Failed {
                receiver,
                pending: Some(chunk),
            };
        }
    }

    loop {
        tokio::select! {
            maybe_chunk = receiver.recv() => {
                match maybe_chunk {
                    Some(chunk) => {
                        // Pipeline flush signals arrive as empty chunks — never
                        // forward them: an empty binary frame tells Deepgram
                        // "end of stream" and would finalize the session early,
                        // clipping the last utterance.
                        if chunk.data.is_empty() {
                            continue;
                        }
                        let pcm = to_linear16(&chunk);
                        if write.send(Message::binary(pcm)).await.is_err() {
                            warn!("Deepgram: send failed mid-session");
                            return SessionEnd::Failed { receiver, pending: Some(chunk) };
                        }
                    }
                    None => break, // recording stopped → finalize below
                }
            }
            maybe_msg = read.next() => {
                match maybe_msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_result(app, text.as_str(), &mut speech_emitted);
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        warn!("Deepgram: server closed the stream mid-session");
                        return SessionEnd::Failed { receiver, pending: None };
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        warn!("Deepgram: websocket read error mid-session: {}", e);
                        return SessionEnd::Failed { receiver, pending: None };
                    }
                }
            }
            _ = keepalive.tick() => {
                // Avoid NET-0001 (10s no-audio timeout) during silence between VAD segments.
                if write.send(Message::text("{\"type\":\"KeepAlive\"}")).await.is_err() {
                    warn!("Deepgram: keepalive send failed mid-session");
                    return SessionEnd::Failed { receiver, pending: None };
                }
            }
        }
    }

    // Recording stopped: ask Deepgram to flush + finalize, then drain the final
    // results with a bounded wait so a hung server can't stall stop_recording.
    let _ = write.send(Message::text("{\"type\":\"CloseStream\"}")).await;
    let drain = async {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => handle_result(app, text.as_str(), &mut speech_emitted),
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    };
    if tokio::time::timeout(DRAIN_TIMEOUT, drain).await.is_err() {
        warn!(
            "Deepgram: final drain timed out after {:?}; closing anyway",
            DRAIN_TIMEOUT
        );
    }
    info!("🌐 Deepgram streaming session ended");
    SessionEnd::Completed
}

/// Convert a VAD AudioChunk (f32, expected 16 kHz) to linear16 little-endian bytes.
/// Borrows the chunk so it can be retried if the send fails.
fn to_linear16(chunk: &AudioChunk) -> Vec<u8> {
    let samples: Cow<'_, [f32]> = if chunk.sample_rate != 16000 {
        // Should never happen — the VAD pipeline emits 16 kHz. Warn once so a
        // misconfiguration is visible without spamming the log.
        static RATE_WARN: std::sync::Once = std::sync::Once::new();
        RATE_WARN.call_once(|| {
            warn!(
                "Deepgram: audio chunk at {} Hz (expected 16000); resampling",
                chunk.sample_rate
            );
        });
        Cow::Owned(crate::audio::audio_processing::resample_audio(
            &chunk.data,
            chunk.sample_rate,
            16000,
        ))
    } else {
        Cow::Borrowed(&chunk.data[..])
    };
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples.iter() {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

// ---- Deepgram result parsing -------------------------------------------------

#[derive(Deserialize)]
struct DgResponse {
    #[serde(default)]
    is_final: bool,
    #[serde(default)]
    channel: Option<DgChannel>,
}

#[derive(Deserialize)]
struct DgChannel {
    #[serde(default)]
    alternatives: Vec<DgAlt>,
}

#[derive(Deserialize)]
struct DgAlt {
    #[serde(default)]
    transcript: String,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    words: Vec<DgWord>,
}

// Only word timings are used (diarize=false → no speaker fields to parse).
#[derive(Deserialize)]
struct DgWord {
    #[serde(default)]
    start: f64,
    #[serde(default)]
    end: f64,
}

fn handle_result<R: Runtime>(app: &AppHandle<R>, payload: &str, speech_emitted: &mut bool) {
    let resp: DgResponse = match serde_json::from_str(payload) {
        Ok(r) => r,
        Err(_) => return, // metadata / non-result messages
    };
    if !resp.is_final {
        return;
    }
    let alt = match resp.channel.and_then(|c| c.alternatives.into_iter().next()) {
        Some(a) => a,
        None => return,
    };
    let text = alt.transcript.trim().to_string();
    if text.is_empty() {
        return;
    }

    let (start, end) = word_span(&alt.words);
    let sequence_id = SEQUENCE_COUNTER.fetch_add(1, Ordering::SeqCst);

    if !*speech_emitted {
        *speech_emitted = true;
        let _ = app.emit(
            "speech-detected",
            serde_json::json!({ "message": "Speech activity detected" }),
        );
    }

    let update = TranscriptUpdate {
        text,
        timestamp: now_hms(),
        source: "Deepgram".to_string(),
        sequence_id,
        chunk_start_time: start,
        is_partial: false,
        confidence: if alt.confidence > 0.0 { alt.confidence } else { 0.9 },
        audio_start_time: start,
        audio_end_time: end,
        duration: (end - start).max(0.0),
        speaker: None, // diarization off
    };

    if let Err(e) = app.emit("transcript-update", &update) {
        error!("Deepgram: failed to emit transcript-update: {}", e);
    }
}

fn word_span(words: &[DgWord]) -> (f64, f64) {
    let start = words.first().map(|w| w.start).unwrap_or(0.0);
    let end = words.last().map(|w| w.end).unwrap_or(start);
    (start, end)
}

fn now_hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}
