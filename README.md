# AIMeeting

Windows-first realtime meeting copilot prototype.

## Current MVP Slice

- Start microphone transcription from the left rail with **Start mic**.
- Cloud ASR Provider is required for microphone transcription. There is no local ASR fallback in the release build.
- Agent Provider is required for Meeting Digest generation and Copilot answers.
- Chinese ASR text is normalized to Simplified Chinese before it enters meeting memory, wake phrase detection, and the copilot context.
- Wake phrases such as `嗨助手` and `hey assistant` start assistant command capture. The app waits for about 4 seconds of silence before filling the Copilot question, and then respects **Auto ask after wake**.
- The main workspace shows a model-generated **Meeting Digest**. Raw ASR segments are kept in a collapsed debug transcript.
- The Copilot can answer, summarize, or plan from the current meeting memory and optional search trail.
- Manual transcript input uses the same wake-phrase path, which makes trigger behavior easy to test without a microphone.

## Run The Desktop Shell

```powershell
npm.cmd install
npm.cmd run desktop:dev
```

## Run The Web Prototype

```powershell
npm.cmd install
npm.cmd run dev
```

Open the Vite URL shown in the terminal. The release ASR path is intended for the Tauri desktop shell because it uses native commands to call the configured cloud ASR endpoint.

## Build

```powershell
npm.cmd run lint
npm.cmd run build
npm.cmd run desktop:build
```

The desktop build generates:

- `src-tauri/target/release/aimeeting.exe`
- `src-tauri/target/release/bundle/msi/AIMeeting_0.1.1_x64_en-US.msi`
- `src-tauri/target/release/bundle/nsis/AIMeeting_0.1.1_x64-setup.exe`

## Provider Configuration

Agent Provider controls text-model work, including Copilot answers and Meeting Digest generation:

- `Base URL`: for example `https://api.openai.com/v1` or an OpenAI-compatible local gateway.
- `Model`: a text model that supports chat completions or responses.
- `Endpoint`: `chat` or `responses`.
- `API key`: kept in memory only by the current prototype; it is not persisted to local storage.

ASR Provider controls microphone transcription:

- `Base URL`: for example `https://api.openai.com/v1` or an OpenAI-compatible cloud gateway.
- `Model`: for example `whisper-1`.
- `API key`: kept in memory only by the current prototype; it is not persisted to local storage.
- All three fields are required before **Start mic** is enabled.

## Known Limitations

- API keys are not persisted. Users must paste them again after restarting the app.
- Speaker labels are still coarse placeholders.
- Meeting memory is still stored in local storage, not SQLite.
- Search logs queries, but no real search endpoint is configured by default.
- Wake phrases are text-level triggers from recognized transcript, not audio-level wake-word detection.
