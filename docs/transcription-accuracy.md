# Centura Capture — transcription accuracy & the audit trail

Live speech-to-text is **not perfect**. Deepgram (and any STT engine) will mishear names, numbers,
and jargon, especially on cross-talk or poor audio. This note records how Centura Capture handles
that risk today.

## Current posture (decided)

- **Synthesis summarizes; it does not fact-check.** The `/synthesize-meetings` step turns the raw
  transcript into notes (summary, decisions, action items). Its prompt forbids inventing content
  ("never invent… use None / omit when unsupported"), but it cannot correct a word the STT got wrong.
  Treat synthesized notes as a **draft record**, not a verified transcript.
- **The verbatim transcript is always archived** at `Meetings/Synthesized/<stem>.md` after synthesis.
  If a note looks off, the source text is there to check.
- **No audio is saved** (M1), so the only artifact to verify against is the text transcript.

## Not enabled (yet)

These were considered and deferred — turn on later if accuracy bites:

- **Keyterm boosting** — give Deepgram a list of firm/people/product names to reduce mishears
  (Deepgram `keyterm` on nova-3). Highest-leverage accuracy improvement.
- **Review gate** — mark synthesized notes `status: needs-review` until a human approves, so nothing
  is auto-trusted as context downstream.

## Guidance

Don't treat a synthesized note as authoritative for compliance-sensitive facts (figures, commitments,
client instructions) without a glance at the archived transcript. If accuracy becomes a recurring
problem, enabling keyterm boosting is the first step.
