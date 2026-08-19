# AIMeeting 0.2.0 发布就绪记录

## 自动验证

- `npm.cmd run check`：通过，包括前端测试、TypeScript、Rust 全套测试、格式和 Clippy。
- 生产前端构建：通过。
- 30 分钟加速双源音频：通过，输出 86,400,000 个 48kHz 采样，录音与 ASR 分支零丢失；工作集约 7.5 MiB。
- 1 分钟真实 Windows 双源录音：通过，工作集稳定约 14 MiB，生成 245,403 字节 Opus；`ffprobe` 解码时长 60.17 秒。
- 30 分钟真实 Windows 双源录音：通过，记录 86,408,640 个采样，工作集保持 13.9-14.3 MiB，生成 7,339,464 字节 Opus；`ffprobe` 确认为 48kHz mono Opus，时长 1800.1865 秒。
- CSP 与 `%LOCALAPPDATA%` 数据目录：真实 Tauri 启动通过。
- 免安装 ZIP 结构与 SHA-256：由 `scripts/verify-portable.ps1` 自动验证，包内共 9 个文件，不含源码、缓存、会议数据或凭据。

## 尚未闭环

- 尚未在干净 Windows 10/11、Windows Sandbox 或独立测试电脑验证启动、WebView2 缺失提示和 Provider 配置。
- 尚未覆盖蓝牙耳机、设备热插拔、磁盘满、企业代理和睡眠唤醒。
- 发布包未数字签名，未进行 SmartScreen 声誉测试。
- 第三方依赖清单已自动生成，但公开商业分发前仍需要许可证与产品自身授权审查。

## 发布定位

`0.2.0` 是供受控测试的 Windows x64 免安装发布候选，不是已签名的公开正式版。ZIP 不包含源码、构建缓存、会议数据、数据库、API Key 或日志。应用数据和凭据仍保存在当前 Windows 用户的系统目录，因此它不是“数据随 EXE 移动”的纯便携软件。
