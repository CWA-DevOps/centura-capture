// audio/vault_export.rs
//
// Centura Capture (M3, redefined): on meeting end, drop the transcript into the
// CenturaOS vault Inbox. A separate Claude Code synthesis step (the
// centura-meeting-synthesis skill) later turns Inbox transcripts into Meeting Notes
// in Notes/ and archives the raw transcript to Synthesized/.
//
//   <root>/Inbox/<stem>.md   -> transcript, frontmatter matches the vault convention
//
// <root> defaults to C:\Users\JakeSteffens\Claude\CenturaOS\Meetings and can be
// overridden with CENTURA_VAULT_ROOT (.env). <stem> = "YYYY.MM.DD - <slug>".

use super::recording_saver::TranscriptSegment;
use anyhow::Result;
use chrono::{Local, SecondsFormat};
use std::fs;
use std::path::PathBuf;

const DEFAULT_VAULT_ROOT: &str = r"C:\Users\JakeSteffens\Claude\CenturaOS\Meetings";
const ATTENDEE_NAME: &str = "Jake Steffens";

pub struct VaultExport {
    pub transcript_path: PathBuf,
}

/// Vault Meetings/ root — CENTURA_VAULT_ROOT override, else the default.
fn vault_root() -> PathBuf {
    match std::env::var("CENTURA_VAULT_ROOT") {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v.trim()),
        _ => PathBuf::from(DEFAULT_VAULT_ROOT),
    }
}

/// Write the transcript for a finished meeting into the vault Inbox.
/// No-op (Ok None) when there are no transcript segments.
pub fn export_meeting(
    meeting_name: &str,
    segments: &[TranscriptSegment],
) -> Result<Option<VaultExport>> {
    // stop_recording detaches the transcript-update listener (and takes the
    // RecordingManager out of its global) before the transcription task drains,
    // so `segments` is missing whatever Deepgram finalized after Stop. Merge in
    // the Deepgram session's own record of every emitted result (dedup by
    // sequence_id; the local fallback path leaves that buffer empty).
    let deepgram_segments = super::transcription::deepgram::take_session_segments();
    let merged = merge_segments(segments, deepgram_segments);

    if merged.is_empty() {
        log::info!("Vault export skipped: no transcript segments");
        return Ok(None);
    }

    let mut ordered: Vec<&TranscriptSegment> = merged.iter().collect();
    ordered.sort_by_key(|s| s.sequence_id);

    let now = Local::now();
    let date = now.format("%Y.%m.%d").to_string();
    let slug = slugify(meeting_name);
    let stem = format!("{} - {}", date, slug);
    let recorded_at = now.to_rfc3339_opts(SecondsFormat::Secs, false);

    // Did the user give a real title, or is this the app's auto timestamp name?
    let title_source = if is_auto_name(meeting_name) { "auto" } else { "user" };

    // Only label speakers when Deepgram actually distinguished more than one.
    let distinct_speakers: std::collections::HashSet<&str> = ordered
        .iter()
        .filter_map(|s| s.speaker.as_deref())
        .collect();
    let diarized = distinct_speakers.len() > 1;

    let title = yaml_escape(meeting_name);
    let body = render_body(&ordered, diarized);

    let transcript_md = format!(
        "---\n\
type: note\n\
title: \"{title}\"\n\
date: {date}\n\
attendees:\n\
  - \"{ATTENDEE_NAME}\"\n\
tags: [meeting, transcript]\n\
source: centura-capture\n\
title_source: {title_source}\n\
recorded_at: {recorded_at}\n\
---\n\n{body}\n"
    );

    let root = vault_root();
    let inbox = root.join("Inbox");
    fs::create_dir_all(&inbox)?;

    // Never overwrite an existing transcript (two same-titled meetings in one
    // day collide on the slug). Suffix " (2)", " (3)"… — matches the synthesis
    // prompt's collision convention.
    let mut transcript_path = inbox.join(format!("{stem}.md"));
    let mut n = 2;
    while transcript_path.exists() {
        transcript_path = inbox.join(format!("{stem} ({n}).md"));
        n += 1;
    }
    fs::write(&transcript_path, transcript_md)?;

    log::info!("📥 Vault export: transcript -> {}", transcript_path.display());

    Ok(Some(VaultExport { transcript_path }))
}

/// Union of the manager's segments and the Deepgram session buffer, keyed by
/// sequence_id. The manager's copy wins on a collision (same data either way).
fn merge_segments(
    manager: &[TranscriptSegment],
    deepgram: Vec<TranscriptSegment>,
) -> Vec<TranscriptSegment> {
    let mut seen: std::collections::HashSet<u64> =
        manager.iter().map(|s| s.sequence_id).collect();
    let mut merged: Vec<TranscriptSegment> = manager.to_vec();
    let mut recovered = 0usize;
    for seg in deepgram {
        if seen.insert(seg.sequence_id) {
            merged.push(seg);
            recovered += 1;
        }
    }
    if recovered > 0 {
        log::info!(
            "Vault export: recovered {} trailing Deepgram segment(s) finalized after Stop",
            recovered
        );
    }
    merged
}

/// True when the meeting name is the app's auto-generated timestamp form, e.g.
/// "Meeting 2026-06-30_09-05-39". Such names should be replaced by a real title
/// during synthesis.
fn is_auto_name(name: &str) -> bool {
    let n = name.trim();
    let rest = match n.strip_prefix("Meeting ") {
        Some(r) => r,
        None => return false,
    };
    // Auto names are "Meeting " + a pure timestamp. Two generators exist:
    //   Rust fallback:      "Meeting 2026-06-30_09-05-39"
    //   Frontend (default): "Meeting 30_06_26_09_05_39"  (useRecordingStart)
    // The old check required a '-', so every frontend-started meeting was
    // mislabeled title_source: user. Accept any all-timestamp remainder.
    rest.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '_' | ':' | ' '))
        && rest.chars().filter(|c| c.is_ascii_digit()).count() >= 8
}

/// Render markdown body. When `diarized`, label by speaker (merging consecutive
/// same-speaker segments); otherwise render plain paragraphs with no speaker labels.
fn render_body(segments: &[&TranscriptSegment], diarized: bool) -> String {
    let mut out = String::new();
    let mut last_speaker: Option<&str> = None;
    for seg in segments {
        let text = seg.text.trim();
        if text.is_empty() {
            continue;
        }
        let speaker = if diarized { seg.speaker.as_deref() } else { None };
        match speaker {
            Some(sp) => {
                if last_speaker != Some(sp) {
                    if !out.is_empty() {
                        out.push_str("\n\n");
                    }
                    out.push_str(&format!("**{}:** {}", sp, text));
                    last_speaker = Some(sp);
                } else {
                    out.push(' ');
                    out.push_str(text);
                }
            }
            None => {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(text);
                last_speaker = None;
            }
        }
    }
    out
}

/// Filename-safe slug: lowercase, non-alphanumeric -> '-', collapsed, trimmed.
fn slugify(input: &str) -> String {
    let mut s = String::with_capacity(input.len());
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            s.push('-');
            prev_dash = true;
        }
    }
    let trimmed = s.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "meeting".to_string()
    } else {
        trimmed
    }
}

/// Minimal YAML double-quoted-string escaping for the title field.
fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
