# AIMeeting

AIMeeting 是一个 Windows 优先的本地会议录音、实时转写和会议纪要桌面应用。`0.2.0` 是供受控测试的免安装发布候选。

## 当前能力

- 麦克风与 Windows 系统声音可独立开关，默认双路混音。
- 录音持续保存为 Ogg Opus；实时转写失败不会停止或丢弃录音。
- 支持开始、暂停、恢复、结束，以及异常退出后的未完成会议恢复。
- DashScope Paraformer 提供低延迟实时字幕。
- 保存后的录音可通过文件 ASR 重新转写。
- 文字模型将转写整理为简体中文会议纪要。
- SQLite 保存会议、转写版本和处理任务；会议历史支持回收站与永久删除。
- API Key 进入 Windows Credential Manager，不写入浏览器存储或 SQLite。

远端多人房间只保留 `RoomGateway` 接口；Android、说话人分离、本地 ASR 和助手唤醒不在当前版本中。

## 使用免安装版

解压 `AIMeeting-0.2.0-windows-x64-no-install.zip` 后运行 `AIMeeting.exe`。完整说明见 [Windows 快速开始](docs/quickstart-windows.md)。

会议数据位于：

```text
%LOCALAPPDATA%\com.aimeeting.app
```

删除 EXE 不会删除会议数据或 Credential Manager 中的 Provider 密钥。Windows 10/11 x64 还需要 Microsoft Edge WebView2 Runtime，系统通常已预装。

## Provider 配置

设置中有三个相互独立的能力：

| 能力 | 当前支持 |
| --- | --- |
| 实时语音转文字 | DashScope `paraformer-realtime-v2` |
| 录音文件转写 | OpenAI-compatible `/audio/transcriptions` 或兼容的 Qwen ASR |
| 会议纪要 | OpenAI-compatible Chat Completions 或 Responses 文字模型 |

远程地址必须使用 HTTPS。本机 `localhost`、`127.0.0.1` 和 `::1` 可以使用 HTTP。

## 源码开发

需要 Node.js、Rust stable、MSVC C++ Build Tools、Windows SDK 和 WebView2 Runtime：

```powershell
npm.cmd install
.\aimeeting.cmd dev
```

`dev` 同时管理 Vite 和 Tauri；关闭该命令后 5173 端口会释放。`web` 只启动前端，不能执行录音、SQLite、凭据等 Tauri 命令。

常用入口：

```powershell
.\aimeeting.cmd check
.\aimeeting.cmd portable
.\aimeeting.cmd release
```

- `check`：前端测试、TypeScript、Rust 测试、格式和 Clippy。
- `portable`：构建并校验 Windows x64 免安装 ZIP 和 SHA-256。
- `release`：构建未签名的 NSIS/MSI 安装包。

## 结构

- `src/`：React 桌面界面、类型化 Tauri bridge 和状态层。
- `src-tauri/src/audio/`：采集、重采样、混音、队列和 Opus 写盘。
- `src-tauri/src/domain/`：会议、任务和 Provider 领域状态。
- `src-tauri/src/gateways/`：实时 ASR、文件 ASR、纪要和 Room Gateway。
- `src-tauri/src/persistence/`：SQLite、文件布局、恢复和凭据存储。
- `src-tauri/src/runtime/`：录音线程、实时转写桥与后台任务恢复。

发布限制和验证证据见 [0.2.0 发布就绪记录](docs/release-readiness-0.2.0.md)。
