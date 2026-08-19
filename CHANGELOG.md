# Changelog

## 0.2.0 - 2026-08-19

### Added

- Windows 麦克风与系统声音双源录制，默认混音并持久化为 Ogg Opus。
- 录音暂停、恢复、结束、异常恢复、会议历史和回收站。
- DashScope Paraformer 实时字幕、录音文件补偿转写和简体中文会议纪要任务。
- SQLite 会议仓库、Windows Credential Manager 密钥存储和旧版文本会议迁移。
- 30 分钟加速音频验证、真实硬件 soak 工具和故障注入矩阵。
- 远端会议房间领域接口，当前版本不开放 UI 或网络实现。

### Changed

- 前端重构为单窗口桌面工作区，只保留录音来源、开始/暂停/结束、转写、纪要、历史和设置。
- 应用数据改存 `%LOCALAPPDATA%\com.aimeeting.app`，首次启动会安全复制旧 Roaming 数据并保留旧副本。
- Provider 远程地址强制 HTTPS，本机回环服务可继续使用 HTTP。

### Security

- 启用 Tauri Content Security Policy。
- 删除废弃的前端分片 ASR IPC，API Key 不进入前端持久化、SQLite 或发布包。

### Known Limitations

- Windows 免安装版尚未数字签名，可能触发 SmartScreen 提示。
- 未提供说话人分离、回声消除、Android 客户端和远端多人房间。
- 实时转写第一版固定使用 DashScope `paraformer-realtime-v2`。
