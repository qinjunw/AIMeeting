# ADR 0001: Single ASR Stream With Meeting Finalization

## Status

Superseded by the native recording and persistent processing design in `0.2.0`.

This ADR is retained as historical context for the earlier wake-assistant prototype. The production UI no longer includes wake capture or Copilot behavior.

## Context

The app needs three overlapping behaviors:

- A stopped Meeting must finish late ASR and digest work without blocking the next Meeting.
- A paused Meeting must stop microphone capture but keep the same Meeting open.
- Assistant wake capture must not steal the meeting text that appears before the wake phrase.

Running separate microphone or ASR pipelines for meeting notes and wake capture would make ownership, permissions, and provider error handling harder to reason about.

## Decision

Use one ASR text stream and dispatch final recognized text to Meeting Digest and Assistant Command Capture consumers. Interim ASR text may be displayed as a Live Caption, but it is not meeting memory. Every async ASR, digest, and Copilot result carries a Meeting id. Late results update their owning Meeting only.

Stop archives the active Meeting and starts a fresh Meeting boundary. Pause stops the current Recording Run but keeps the Meeting active.

For the release build, this single ASR stream is backed by the configured cloud ASR Provider. Local ASR is not exposed as a fallback path.

## Consequences

- Users can stop and immediately start a new Meeting while the old Meeting finalizes in the background.
- Late ASR and digest results cannot leak into the next active Meeting.
- Wake capture remains text-level detection for the MVP, not audio-level wake-word detection.
- Cloud ASR and Agent Provider configuration failures should be surfaced explicitly instead of silently degrading to local or hard-coded output.
