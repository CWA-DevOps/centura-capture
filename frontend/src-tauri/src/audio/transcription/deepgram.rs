// audio/transcription/deepgram.rs
//
// Centura Capture (M2): Deepgram live streaming STT backend.
//
// Opens one WebSocket per meeting to Deepgram's streaming API WITHOUT diarization
// (diarize=false — plain transcript; speaker labels were unreliable) and no-retention
// (mip_opt_out=true). Audio frames come from the
// existing VAD AudioChunk receiver (16 kHz mono f32), are converted to linear16,
// and streamed up. Final results are parsed (text + speaker label) and emitted as
// the same `transcript-update` event the local engines use, so the frontend is
// unchanged apart from the new optional `speaker` field.
//
// Fallback: if Deepgram is not the selected provider, no key is present, or the
// WebSocket fails to connect, `maybe_run_deepgram` hands the receiver back so the
// caller can run the local engine instead.

use super::worker::TranscriptUpdate;
use crate::audio::AudioChunk;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_MODEL: &str = "nova-3";
const LANGUAGE: &str = "en-US";

// Sequence counter for Deepgram-emitted transcript updates (separate from the
// local worker's counter; the two paths never run simultaneously).
static SEQUENCE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Outcome of the Deepgram branch in `start_transcription_task`.
pub enum DeepgramOutcome {
    /// Deepgram ran the whole session and consumed the receiver.
    Handled,
    /// Deepgram is not used (not selected / no key / connect failed); the caller
    /// should run the local engine with the returned receiver.
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

/// If the configured provider is Deepgram and a key is available, connect and run
/// the streaming session. Otherwise (or on connect failure) return the receiver
/// for local fallback.
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

    match connect(&key, &model).await {
        Ok(ws) => {
            info!("✅ Deepgram WebSocket connected");
            run_session(app, receiver, ws).await;
            DeepgramOutcome::Handled
        }
        Err(e) => {
            error!("❌ Deepgram connect failed: {} — falling back to local STT", e);
            let _ = app.emit(
                "transcription-warning",
                format!("Deepgram unavailable ({}). Using local transcription.", e),
            );
            DeepgramOutcome::FallBack(receiver)
        }
    }
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Open the Deepgram streaming WebSocket with the Authorization header.
async fn connect(key: &str, model: &str) -> Result<WsStream, String> {
    let url = format!(
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

/// Run the full streaming session: stream audio up, emit transcripts down,
/// until the receiver closes (recording stopped), then finalize.
async fn run_session<R: Runtime>(
    app: AppHandle<R>,
    mut receiver: UnboundedReceiver<AudioChunk>,
    ws: WsStream,
) {
    let (mut write, mut read) = ws.split();

    // Sender task: pump audio frames + keepalives; finalize on stop.
    let sender = tokio::spawn(async move {
        let mut keepalive = tokio::time::interval(Duration::from_secs(5));
        keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                maybe_chunk = receiver.recv() => {
                    match maybe_chunk {
                        Some(chunk) => {
                            let pcm = to_linear16(chunk);
                            if write.send(Message::binary(pcm)).await.is_err() {
                                warn!("Deepgram: send failed, stopping audio pump");
                                break;
                            }
                        }
                        None => {
                            // Recording stopped: ask Deepgram to flush + finalize.
                            let _ = write.send(Message::text("{\"type\":\"CloseStream\"}")).await;
                            break;
                        }
                    }
                }
                _ = keepalive.tick() => {
                    // Avoid NET-0001 (10s no-audio timeout) during silence between VAD segments.
                    if write.send(Message::text("{\"type\":\"KeepAlive\"}")).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = write.flush().await;
    });

    // Reader loop: parse results and emit transcript-update.
    let mut speech_emitted = false;
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                handle_result(&app, text.as_str(), &mut speech_emitted);
            }
            Ok(Message::Binary(_)) => { /* Deepgram sends JSON text; ignore binary */ }
            Ok(Message::Close(_)) => {
                info!("Deepgram: server closed the stream");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                warn!("Deepgram: websocket read error: {}", e);
                break;
            }
        }
    }

    let _ = sender.await;
    info!("🌐 Deepgram streaming session ended");
}

/// Convert a VAD AudioChunk (f32, possibly non-16k) to linear16 little-endian bytes.
fn to_linear16(chunk: AudioChunk) -> Vec<u8> {
    let samples = if chunk.sample_rate != 16000 {
        crate::audio::audio_processing::resample_audio(&chunk.data, chunk.sample_rate, 16000)
    } else {
        chunk.data
    };
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
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

#[derive(Deserialize)]
struct DgWord {
    #[serde(default)]
    speaker: Option<u32>,
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

    let speaker = dominant_speaker(&alt.words);
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
        speaker,
    };

    if let Err(e) = app.emit("transcript-update", &update) {
        error!("Deepgram: failed to emit transcript-update: {}", e);
    }
}

/// Most frequent speaker index among the words → "Speaker {n+1}" (1-based for users).
fn dominant_speaker(words: &[DgWord]) -> Option<String> {
    use std::collections::HashMap;
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for w in words {
        if let Some(s) = w.speaker {
            *counts.entry(s).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(s, _)| format!("Speaker {}", s + 1))
}

fn word_span(words: &[DgWord]) -> (f64, f64) {
    let start = words.first().map(|w| w.start).unwrap_or(0.0);
    let end = words.last().map(|w| w.end).unwrap_or(start);
    (start, end)
}

fn now_hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}
