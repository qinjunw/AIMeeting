# AIMeeting Windows V1 Productization Implementation Plan

> **For Codex:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to execute this plan task-by-task. Use test-driven development for behavioral changes and verification-before-completion before every commit.

**Goal:** Turn the existing realtime-ASR prototype into a stable Windows desktop meeting recorder that persists microphone/system audio, degrades safely when AI services fail, produces simplified-Chinese transcripts and minutes, and ships as a portable ZIP.

**Architecture:** Keep Tauri 2 + React, but move recording, persistence, provider calls, and lifecycle state into Rust. React becomes a typed desktop UI over narrow Tauri commands/events. A crash-safe local recording is the source of truth; live ASR and minutes generation are independent persistent jobs.

**Tech Stack:** Tauri 2, React 19, TypeScript 6, Rust 2021, Tokio, CPAL/WASAPI, pure-Rust Opus + Ogg, rusqlite, reqwest, tokio-tungstenite, Vitest, React Testing Library.

**Approved specification:** `docs/superpowers/specs/2026-08-19-aimeeting-product-v1-design.md`

**Branch:** `codex/windows-v1-productization`

**Commit policy:** One Chinese commit per verified milestone. Never combine unrelated phases, never push, and never create a release tag until automated checks and the documented hardware test matrix pass.

---

## Task 1: Freeze the specification and verification entry point

**Files:**
- Add: `docs/superpowers/specs/2026-08-19-aimeeting-product-v1-design.md`
- Add: `docs/superpowers/plans/2026-08-19-aimeeting-windows-v1-implementation.md`
- Modify: `CONTEXT.md`
- Add/Modify: `aimeeting.cmd`
- Modify: `.gitignore`
- Modify: `package.json`

**Steps:**
1. Preserve the approved product decisions and domain glossary.
2. Add `check`, `test`, `test:frontend`, `test:rust`, `format:check`, and `clippy` scripts without changing runtime behavior.
3. Add Vitest/jsdom dependencies and configuration.
4. Make `aimeeting.cmd check` run frontend tests, TypeScript, Rust tests, rustfmt check, and Clippy.
5. Ignore `.env`, coverage, test reports, release staging, and soak-test output.
6. Run the existing baseline before and after the tooling change.

**Verification:**
```powershell
npm.cmd run lint
npm.cmd run build
cargo test --manifest-path src-tauri\Cargo.toml
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets -- -D warnings
```

**Commit:**
```text
docs: 固化 Windows V1 产品规格并建立统一验证入口
```

## Task 2: Extract the Rust ASR module without behavioral changes

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Add: `src-tauri/src/commands/mod.rs`
- Add: `src-tauri/src/gateways/mod.rs`
- Add: `src-tauri/src/gateways/live_asr/mod.rs`
- Add: `src-tauri/src/gateways/live_asr/dashscope.rs`
- Add: `src-tauri/src/gateways/file_asr/mod.rs`
- Add: `src-tauri/src/gateways/file_asr/openai_compatible.rs`

**Steps:**
1. Move current streaming DTOs, WebSocket task, parser, and tests into `gateways/live_asr/dashscope.rs`.
2. Move chunk/file HTTP transcription into `gateways/file_asr/openai_compatible.rs`.
3. Keep command names and event payloads unchanged so the current UI still runs.
4. Make ASR status a serialized enum and ensure every natural WebSocket close emits `finished` or `error`.
5. Add a finish timeout test and a natural-close parser test before changing behavior.
6. Keep `lib.rs` as application assembly and command registration only.

**Verification:**
```powershell
cargo test --manifest-path src-tauri\Cargo.toml gateways
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets -- -D warnings
npm.cmd run build
```

**Commit:**
```text
refactor: 拆分实时与文件转写模块并收紧会话结束语义
```

## Task 3: Add the meeting domain state machine

**Files:**
- Add: `src-tauri/src/domain/mod.rs`
- Add: `src-tauri/src/domain/meeting.rs`
- Add: `src-tauri/src/domain/jobs.rs`
- Add: `src-tauri/src/domain/provider.rs`
- Add: `src-tauri/src/meeting/mod.rs`
- Add: `src-tauri/src/meeting/service.rs`

**Steps:**
1. Write failing tests for `preparing -> recording <-> paused -> stopping -> processing -> ready`.
2. Add invalid-transition, idempotent-stop, interrupted-recovery, and generation-isolation tests.
3. Implement `MeetingRecord`, `RecordingRun`, `RecordingStatus`, `TranscriptionStatus`, `MinutesStatus`, and revision-bearing job types.
4. Implement `MeetingService` over repository/audio/gateway traits using fakes in tests.
5. Prove the invariant: ASR failure changes only transcription state and never stops recording.

**Verification:**
```powershell
cargo test --manifest-path src-tauri\Cargo.toml domain meeting
```

**Commit:**
```text
test: 建立会议录音状态机和异步任务隔离基线
```

## Task 4: Introduce SQLite and crash-safe file layout

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Add: `src-tauri/src/persistence/mod.rs`
- Add: `src-tauri/src/persistence/database.rs`
- Add: `src-tauri/src/persistence/migrations.rs`
- Add: `src-tauri/src/persistence/files.rs`
- Add: `src-tauri/src/persistence/recovery.rs`
- Add: `src-tauri/migrations/0001_initial.sql`

**Steps:**
1. Add `rusqlite` with `bundled`, `thiserror`, `chrono`, and `tempfile` for tests.
2. Write migration tests for all approved tables and indexes.
3. Write repository tests for create/list/update, transcript revisions, job leases, soft delete, restore, and permanent delete.
4. Implement `%LOCALAPPDATA%\AIMeeting` paths with injectable temporary roots for tests.
5. Implement atomic temporary-file rename and startup scan for unfinished meetings.
6. Keep database writes transactional and never delete audio before the database transaction permits it.

**Verification:**
```powershell
cargo test --manifest-path src-tauri\Cargo.toml persistence
```

**Commit:**
```text
feat: 引入 SQLite 会议仓库和可恢复文件布局
```

## Task 5: Build deterministic audio primitives and Ogg Opus writer

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Add: `src-tauri/src/audio/mod.rs`
- Add: `src-tauri/src/audio/frame.rs`
- Add: `src-tauri/src/audio/resampler.rs`
- Add: `src-tauri/src/audio/mixer.rs`
- Add: `src-tauri/src/audio/preprocessor.rs`
- Add: `src-tauri/src/audio/ogg_opus.rs`
- Add: `src-tauri/src/audio/fake.rs`

**Steps:**
1. Write tests for mono conversion, linear resampling, source gain, limiter, silence level, and timestamp ordering.
2. Write a 30-minute accelerated synthetic test proving bounded buffers and stable sample counts with clock drift.
3. Add a pure-Rust Opus encoder and Ogg packet writer; do not introduce a runtime DLL dependency.
4. Write valid `OpusHead`/`OpusTags`, 20 ms packets, granule positions, periodic page flushes, and `sync_data` checkpoints.
5. Encode each Recording Run as a valid recoverable Ogg logical stream; concatenate completed runs as an Ogg chained stream for one user-facing file.
6. Test normal finalization, pause/resume chaining, truncated current part recovery, and output duration.

**Verification:**
```powershell
cargo test --manifest-path src-tauri\Cargo.toml audio
```

**Commit:**
```text
feat: 实现可测试混音管线和崩溃安全的 Opus 录音写入
```

## Task 6: Implement Windows microphone and system loopback capture

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Add: `src-tauri/src/audio/capture.rs`
- Add: `src-tauri/src/audio/engine.rs`
- Add: `src-tauri/src/audio/platform/mod.rs`
- Add: `src-tauri/src/audio/platform/windows/mod.rs`
- Add: `src-tauri/src/audio/platform/windows/cpal_capture.rs`
- Add: `src-tauri/src/bin/audio_probe.rs`

**Steps:**
1. Add CPAL 0.18 and an `AudioCaptureSource` trait.
2. Enumerate stable device IDs, names, native formats, and default microphone/output devices.
3. Build microphone input from an input endpoint and whole-system loopback from an output endpoint used as CPAL input on WASAPI.
4. Convert native `f32/i16/u16` callbacks to timestamped frames without blocking the audio callback.
5. Add RMS/activity checks so a callback that only produces near-zero samples is reported as degraded rather than treated as healthy.
6. Feed both sources through bounded queues into the mixer, recorder, and independent best-effort ASR sink.
7. Add `audio_probe` for 10-second microphone, loopback, and mixed diagnostic captures.
8. If CPAL fails the documented USB/Bluetooth/default-device checks, replace the entire Windows adapter with direct WASAPI under the same trait; do not mix half implementations.

**Verification:**
```powershell
cargo test --manifest-path src-tauri\Cargo.toml audio
cargo run --manifest-path src-tauri\Cargo.toml --bin audio_probe -- --list
```

**Commit:**
```text
feat: 接入 Windows 麦克风和系统声音双源采集
```

## Task 7: Expose recording lifecycle commands and decouple live ASR

**Files:**
- Add: `src-tauri/src/commands/recording.rs`
- Add: `src-tauri/src/commands/meetings.rs`
- Add: `src-tauri/src/runtime/mod.rs`
- Add: `src-tauri/src/runtime/registry.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/gateways/live_asr/dashscope.rs`

**Steps:**
1. Write integration tests for start, pause, resume, stop, rapid stop/start, and stale event isolation using fake audio/ASR.
2. Add commands: `list_audio_devices`, `start_recording`, `pause_recording`, `resume_recording`, `stop_recording`, and `get_active_meeting`.
3. Start local recording before attempting ASR connection.
4. Route mixed 16 kHz PCM frames directly from Rust audio engine to the live gateway; remove Base64 audio IPC from the product path.
5. Stop order: stop capture, drain mixer, sync/finalize audio, commit recording state, then wait for ASR final with a bounded timeout.
6. Emit typed `meeting-state-event`, `recording-level-event`, and `transcription-event` payloads.

**Verification:**
```powershell
cargo test --manifest-path src-tauri\Cargo.toml recording_lifecycle
npm.cmd run build
```

**Commit:**
```text
feat: 建立录音优先的生命周期命令并解耦实时转写
```

## Task 8: Implement provider profiles, file retry, and minutes jobs

**Files:**
- Add: `src-tauri/src/gateways/traits.rs`
- Add: `src-tauri/src/gateways/minutes/mod.rs`
- Add: `src-tauri/src/gateways/minutes/openai_compatible.rs`
- Add: `src-tauri/src/jobs/mod.rs`
- Add: `src-tauri/src/jobs/runner.rs`
- Add: `src-tauri/src/persistence/secrets.rs`
- Add: `src-tauri/src/commands/providers.rs`
- Add: `src-tauri/src/commands/jobs.rs`

**Steps:**
1. Define `LiveTranscriptionGateway`, `FileTranscriptionGateway`, and `MinutesGateway` capability traits.
2. Store non-secret provider profiles in SQLite and secrets in Windows Credential Manager/keyring storage.
3. Move text-model HTTP calls from the WebView into Rust with cancellation and timeouts.
4. Implement persistent file-transcription and minutes jobs with retry count, error summary, and transcript revision checks.
5. Coalesce minutes updates per meeting and reject stale revision writes.
6. Add commands for provider CRUD/test, `retry_transcription`, `retry_minutes`, and processing-job status.
7. Ensure all minutes output passes simplified-Chinese normalization and preserves facts/action items.

**Verification:**
```powershell
cargo test --manifest-path src-tauri\Cargo.toml gateways jobs
```

**Commit:**
```text
feat: 统一 AI Gateway 并支持文件重转写和纪要重试
```

## Task 9: Create typed frontend bridge and state reducers

**Files:**
- Modify: `src/types.ts`
- Add: `src/domain/meeting.ts`
- Add: `src/bridge/recordingClient.ts`
- Add: `src/bridge/meetingRepositoryClient.ts`
- Add: `src/bridge/providerClient.ts`
- Add: `src/features/recording/recordingMachine.ts`
- Add: `src/features/transcription/transcriptReducer.ts`
- Add: `src/features/meeting/useMeetingSession.ts`
- Add: `src/features/history/useMeetingHistory.ts`
- Add: `src/features/settings/useProviderSettings.ts`
- Add tests beside each reducer/hook.

**Steps:**
1. Write frontend state-machine tests before implementation.
2. Separate recording, transcription, and minutes states.
3. Make bridge functions the only frontend files allowed to call Tauri `invoke`/`listen`.
4. Subscribe once under React StrictMode and cleanly unlisten.
5. Ignore late events whose meeting/run/revision does not match their owning record.
6. Remove all Web Audio, PCM resampling, Base64 conversion, and direct Provider fetches from React.

**Verification:**
```powershell
npm.cmd run test:frontend
npm.cmd run lint
```

**Commit:**
```text
refactor: 建立类型化桌面桥接和前端会议状态层
```

## Task 10: Replace the prototype with the minimal desktop UI

**Files:**
- Replace: `src/App.tsx`
- Replace: `src/index.css`
- Add: `src/app/AppShell.tsx`
- Add: `src/features/history/MeetingSidebar.tsx`
- Add: `src/features/recording/RecordingBar.tsx`
- Add: `src/features/meeting/MeetingWorkspace.tsx`
- Add: `src/features/meeting/MinutesView.tsx`
- Add: `src/features/transcription/TranscriptView.tsx`
- Add: `src/features/settings/SettingsDialog.tsx`
- Add: `src/features/trash/TrashDialog.tsx`
- Add: `src/styles/tokens.css`
- Add: `src/styles/shell.css`

**Steps:**
1. Write component tests for button availability, default source toggles, history selection, tabs, retry states, and dialogs.
2. Implement a fixed desktop shell: history sidebar, meeting workspace, fixed recording bar.
3. Keep only `会议纪要` and `完整转写` tabs.
4. Make microphone/system source controls default on and editable while idle/paused.
5. Show ASR failure as a non-blocking status with a retry action.
6. Remove wake phrase, Copilot, search, evidence, manual segments, probes, metrics, and explanatory copy from production UI.
7. Keep native window chrome, compact typography, restrained multi-color status palette, and no nested cards.
8. Verify 1100x740 minimum, 1280x720, 1440x900, and high-DPI layouts with screenshots.

**Verification:**
```powershell
npm.cmd run test:frontend
npm.cmd run build
```

**Commit:**
```text
feat: 重做极简桌面录音界面和会议工作区
```

## Task 11: Add history, recycle bin, recovery, and legacy import

**Files:**
- Modify: `src-tauri/src/commands/meetings.rs`
- Modify: `src-tauri/src/persistence/recovery.rs`
- Add: `src-tauri/src/persistence/legacy_import.rs`
- Modify: `src/features/history/useMeetingHistory.ts`
- Modify: `src/features/history/MeetingSidebar.tsx`
- Modify: `src/features/trash/TrashDialog.tsx`

**Steps:**
1. Write tests for fixed-size paged/scrolling history, selection, soft delete, restore, permanent delete, and trash size.
2. Add list/get/rename/trash/restore/permanent-delete commands.
3. Move meeting directories to trash only after database state is committed; compensate safely on file errors.
4. Recover interrupted meetings on startup and surface them as resumable/finalizable records.
5. Add one-time import of legacy localStorage records as text-only meetings without deleting source data automatically.
6. Ensure no history operation can affect an active recording.

**Verification:**
```powershell
cargo test --manifest-path src-tauri\Cargo.toml persistence meeting
npm.cmd run test:frontend
```

**Commit:**
```text
feat: 完善会议历史回收站异常恢复和旧数据迁移
```

## Task 12: Add fault injection and reliability tests

**Files:**
- Add: `src-tauri/tests/recording_pipeline.rs`
- Add: `src-tauri/tests/recovery.rs`
- Add: `src-tauri/tests/provider_failures.rs`
- Add: `src-tauri/src/bin/recording_soak.rs`
- Add: `docs/testing/windows-audio-matrix.md`
- Add: `docs/testing/soak-test.md`

**Steps:**
1. Test fake ASR disconnect, bad key, queue saturation, slow consumer, late final, and file-write failure.
2. Prove ASR queue saturation never loses recorder frames.
3. Run an accelerated synthetic 30-minute pipeline and validate samples, queue bounds, revisions, and output duration.
4. Add a real-time soak runner that logs memory, CPU, threads, queue depth, dropped frames, and file size every five seconds.
5. Run real device tests for microphone only, system only, and mixed capture where hardware access permits.
6. Document any unverified hardware matrix rows explicitly rather than marking them passed.

**Verification:**
```powershell
cargo test --manifest-path src-tauri\Cargo.toml --all-targets
cargo run --release --manifest-path src-tauri\Cargo.toml --bin recording_soak -- --synthetic-minutes 30
```

**Commit:**
```text
test: 覆盖三十分钟录音和云端故障降级场景
```

## Task 13: Harden the desktop app and build a portable ZIP

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `README.md`
- Replace: `docs/release-windows.md`
- Add: `docs/privacy.md`
- Add: `THIRD_PARTY_LICENSES.md`
- Add: `scripts/build-portable.ps1`
- Add: `scripts/verify-portable.ps1`
- Modify: `aimeeting.cmd`

**Steps:**
1. Set product metadata, minimum window size, CSP, and only required Tauri capabilities.
2. Add a WebView2 availability/startup diagnostic with actionable text.
3. Build `--no-bundle`, stage only EXE/docs/licenses, generate SHA-256, and produce `AIMeeting-<version>-windows-x64-portable.zip`.
4. Do not include source, PDB, `target`, `node_modules`, credentials, recordings, or logs.
5. Document first launch, data directory, BYOK setup, cloud-audio privacy, SmartScreen, and deletion semantics.
6. Verify the extracted package from a path containing Chinese characters and spaces.

**Verification:**
```powershell
powershell -ExecutionPolicy Bypass -File scripts\build-portable.ps1
powershell -ExecutionPolicy Bypass -File scripts\verify-portable.ps1
```

**Commit:**
```text
build: 增加 Windows 便携包构建校验和发布说明
```

## Task 14: Whole-product review and release-candidate checkpoint

**Files:**
- Modify only files required by review findings.
- Add: `CHANGELOG.md`
- Add: `docs/release-readiness-0.2.0.md`

**Steps:**
1. Run the complete verification suite from a clean process.
2. Review the full branch against every specification acceptance criterion.
3. Use a separate review Agent for Rust safety/concurrency and another for desktop UX/data loss risks.
4. Fix all Critical/Important findings, rerun scoped tests, then rerun the full suite.
5. Record automated results and manual hardware rows separately.
6. Build the portable candidate and record artifact path, size, and SHA-256.
7. Do not create a public release tag or push; leave the branch with local verified commits.

**Verification:**
```powershell
aimeeting.cmd check
npm.cmd run build
npm.cmd run desktop:build -- --no-bundle
powershell -ExecutionPolicy Bypass -File scripts\build-portable.ps1
git status --short
```

**Commit:**
```text
release: 完成 AIMeeting Windows 0.2.0 便携候选版检查
```

---

## Execution rules

- Use RED -> GREEN -> REFACTOR for every new behavior.
- Before every commit: inspect `git diff`, run the phase tests, run TypeScript/Rust format checks applicable to touched files, and confirm no key or recording entered Git.
- Parallel Agents may own disjoint files or perform read-only review. The controller integrates and verifies every change before committing.
- Never stop recording because an ASR/minutes Provider fails.
- Never block Stop indefinitely on network finalization.
- Never write API keys to localStorage, SQLite plaintext, logs, or events.
- Never delete user audio except through an explicit permanent-delete transaction.
- Never push to a remote in this plan.
