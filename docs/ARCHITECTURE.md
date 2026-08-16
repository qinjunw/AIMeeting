# 架构说明

| 层 | 目录 | 职责 |
| --- | --- | --- |
| React UI | `src/App.tsx` | 会议状态、实时转写、摘要、Copilot 和设置 |
| 前端服务 | `src/services/` | 音频采集、文本规范化、会议记忆、模型调用和搜索策略 |
| 本地存储 | `src/lib/storage.ts` | 会议片段、摘要和界面配置 |
| Tauri bridge | `src-tauri/src/lib.rs` | DashScope WebSocket 会话、音频帧传输和事件回推 |

## 实时链路

1. 用户授权麦克风，前端按 16 kHz PCM16 生成音频帧。
2. Tauri 通过 DashScope WebSocket 维护流式 ASR 会话。
3. interim/final 事件回到 React，文本经规范化后写入会议时间线。
4. 增量摘要器只读取当前会议片段；Copilot 将会议证据与用户问题发送给文本模型。
5. 停止会议后，片段、摘要和问答结果归档到本地历史。

## 数据与安全边界

- ASR Key 和文本模型 Key 不写入 localStorage，但仍存在于当前进程内存。
- 会议历史以明文保存在浏览器/Tauri WebView 本地存储，不适合敏感会议的生产部署。
- Tauri CSP 已限制为本地资源、IPC、HTTPS/WSS Provider 连接和必要的内联样式。
- 搜索默认关闭；外部搜索结果和会议事实必须在回答中保持来源分离。
