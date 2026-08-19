# ADR 0002: DashScope Streaming ASR for V1

## Status

Accepted

## Context

The existing ASR path uploads short audio chunks to a cloud HTTP endpoint and receives complete text per chunk. That improves throughput with concurrency, but it still feels blocked because the user sees no provider interim result while speaking.

The product direction is to stabilize low-latency speech-to-text while making saved audio the durable source of truth. Meeting-minutes generation runs from persisted transcript revisions.

## Decision

Use DashScope Paraformer realtime WebSocket as the first true streaming ASR path. Keep the WebSocket connection in the Tauri backend so the frontend never opens a provider socket with the API key.

For V1, implement this as a DashScope-specific provider instead of a generic streaming-ASR abstraction. The native recording worker emits bounded 16kHz PCM16 packets to the Rust gateway, which forwards binary audio to DashScope and emits ASR lifecycle and transcript events to the frontend. The recorder never waits for the network consumer.

Interim ASR Results are Live Caption only. Final ASR Results are persisted before notification and become transcript revisions used by Meeting Minutes.

The previous frontend chunked HTTP ASR IPC is removed from the release surface. After recording ends, a persistent saved-file transcription job provides an explicit retry path without affecting the recording asset.

## Consequences

- The ASR layer can show text while the user is still speaking.
- Stop and Pause close or freeze local capture promptly; cloud completion continues under the owning Meeting without blocking a new recording.
- Late final results are routed by Meeting id and Recording Run id, so they cannot leak into the next Meeting.
- Adding another realtime provider later will require a new adapter or abstraction after DashScope latency and stability are measured.
