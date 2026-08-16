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
4. 配置 Agent Provider 后，增量摘要只读取当前会议片段；Copilot 将会议证据与用户问题发送给外部文本模型。
5. 停止会议时先归档当前快照并建立新会议边界；迟到的 ASR final 和纪要结果在后台更新其所属历史会议。

## 数据与安全边界

- ASR Key 和文本模型 Key 不写入 localStorage，但仍存在于当前进程内存。
- 会议历史以明文保存在浏览器/Tauri WebView 本地存储，不适合敏感会议的生产部署。
- Tauri CSP 已限制为本地资源、IPC、HTTPS/WSS Provider 连接和必要的内联样式。
- 搜索默认关闭；外部搜索结果和会议事实必须在回答中保持来源分离。

公开版只提供麦克风捕获。作品截图中出现的 System probe、系统声和混合来源统计属于完整本地原型展示，详见 [作品集公开版说明](PORTFOLIO_EDITION.md)。
