# ADR 0001: Single ASR Stream With Meeting Finalization

## Status

Accepted

## Context

The app needs three overlapping behaviors:

- A stopped Meeting must finish late ASR and digest work without blocking the next Meeting.
- A paused Meeting must stop microphone capture but keep the same Meeting open.
- Assistant wake capture must not steal the meeting text that appears before the wake phrase.

Running separate microphone or ASR pipelines for meeting notes and wake capture would make ownership, permissions, and local Whisper load harder to reason about.

## Decision

Use one ASR text stream and dispatch recognized text to Meeting Digest and Assistant Command Capture consumers. Every async ASR, digest, and Copilot result carries a Meeting id. Late results update their owning Meeting only.

Stop archives the active Meeting and starts a fresh Meeting boundary. Pause stops the current Recording Run but keeps the Meeting active.

## Consequences

- Users can stop and immediately start a new Meeting while the old Meeting finalizes in the background.
- Late ASR and digest results cannot leak into the next active Meeting.
- Wake capture remains text-level detection for the MVP, not audio-level wake-word detection.
