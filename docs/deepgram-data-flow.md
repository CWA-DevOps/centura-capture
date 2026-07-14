# Centura Capture — Deepgram data flow (M2)

For compliance review before Deepgram is used in real client meetings.

## What leaves the device

When the transcription provider is Deepgram, Centura Capture streams meeting **audio** to
Deepgram's cloud over a TLS WebSocket (`wss://api.deepgram.com/v1/listen`) and receives text
transcripts back. No audio is written to local disk (M1 holds). No audio file is sent to Deepgram —
only the live PCM stream needed for transcription.

- **Audio content:** mixed microphone + system audio, 16 kHz mono.
- **Destination:** Deepgram's API (United States region).
- **Returned:** plain text transcript with **no speaker labels** (`diarize=false`). Server-side
  diarization was tried and turned off — the labels were unreliable.
- **Also sent:** the request URL carries the firm-vocabulary keyterm list from `DEEPGRAM_KEYTERMS`
  in `.env` (names, vendors, jargon used to bias recognition). Treat the list itself as data that
  leaves the device; keep client names out of it until client use is cleared.

## Retention / training controls

Every request sets **`mip_opt_out=true`**, which excludes the audio from Deepgram's Model
Improvement Program. Per Deepgram's documentation, opted-out audio is retained only for the time
needed to process the request and is not used to train models.

- **Cost impact:** opting out forgoes Deepgram's 50% Model Improvement discount, so per-minute cost
  is roughly double the standard rate. Confirm against the current Deepgram price sheet.
- **To verify before client use:** whether account-level zero-retention and a signed agreement
  (BAA/DPA) are also required for the firm's data classes.

## Key handling

The Deepgram API key is read from a local, gitignored `.env` (`DEEPGRAM_API_KEY`) in
`frontend/src-tauri/`. It is never committed and never stored in the app database for Deepgram.

## Fallback

If no key is present, or Deepgram is unreachable, transcription falls back to the on-device local
engine (Parakeet). Local transcription sends nothing to the cloud.

## Open items for compliance

1. Confirm sending client meeting audio to Deepgram (US) is acceptable for the relevant data classes.
2. Confirm whether a BAA/DPA with Deepgram is required and in place.
3. Confirm the `mip_opt_out` no-retention behavior against Deepgram's current terms.
