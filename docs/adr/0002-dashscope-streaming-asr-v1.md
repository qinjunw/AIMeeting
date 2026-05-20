# ADR 0002: DashScope Streaming ASR for V1

## Status

Accepted

## Context

The existing ASR path uploads short audio chunks to a cloud HTTP endpoint and receives complete text per chunk. That improves throughput with concurrency, but it still feels blocked because the user sees no provider interim result while speaking.

The product direction is to stabilize low-latency speech-to-text first, then evaluate mature meeting-minutes generation approaches, and only later revisit Copilot wake behavior.

## Decision

Use DashScope Paraformer realtime WebSocket as the first true streaming ASR path. Keep the WebSocket connection in the Tauri backend so the frontend never opens a provider socket with the API key.

For V1, implement this as a DashScope-specific provider instead of a generic streaming-ASR abstraction. The frontend sends small PCM16 frames to Tauri, Tauri forwards binary audio to DashScope, and Tauri emits ASR lifecycle and transcript events back to the frontend.

Interim ASR Results are Live Caption only. Final ASR Results become Raw ASR Segments and Meeting Digest input.

The previous chunked HTTP ASR path may remain as a development comparison path, but it is not the release fallback and must not silently replace a running streaming session.

## Consequences

- The ASR layer can show text while the user is still speaking.
- Stop and Pause must wait for the owning Streaming ASR Session to finish before treating that Recording Run as ASR-idle.
- Late final results are routed by Meeting id and Recording Run id, so they cannot leak into the next Meeting.
- Adding another realtime provider later will require a new adapter or abstraction after DashScope latency and stability are measured.
