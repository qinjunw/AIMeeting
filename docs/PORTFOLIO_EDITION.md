# 作品集公开版说明

## 仓库定位

AIMeeting 是一个可运行、可验证的 **source-available portfolio** 仓库，用于展示 Windows 会议 Copilot 的产品原型和关键技术链路。它是完整本地原型中适合公开审阅的子集，不等于经过安全、合规、可靠性和运维验证的生产系统。

## 公开源码可运行或验证的能力

| 能力 | 当前公开版的验证边界 |
| --- | --- |
| Web 界面 | 可启动完整交互界面，手工加入转写片段，查看原始时间线、状态和来源计数。 |
| 文本处理 | 手工输入和 ASR final 文本会做空白归一化与简体中文转换；文本级唤醒短语可拆分会议内容和助手指令。 |
| 会议生命周期 | Pause 保留当前会议，Stop 建立新会议边界；片段、纪要和问答结果可归档到本地历史并删除。 |
| 麦克风流式 ASR | 在 Tauri 桌面模式下，以 16 kHz mono PCM16 向 DashScope Paraformer realtime WebSocket 发送麦克风音频，并处理 interim/final 事件。需要审阅者自己的 DashScope Key 和可用网络。 |
| 增量纪要与 Copilot | 配置 OpenAI-compatible Agent Provider 后，可按新增片段更新纪要，并基于最近会议片段生成回答、计划项和可展开的会议证据。模型质量和可用性由外部 Provider 决定。 |
| 搜索适配器 | 代码包含默认关闭的通用 HTTP 搜索 endpoint 适配器、查询记录和基础脱敏。实际结果依赖审阅者提供兼容 endpoint，并受网络、CORS 和返回格式约束。 |

公开 UI 的音频来源固定为 `microphone`。类型和合成演示数据中保留的 `system` / `mixed` 标签不构成系统音频捕获能力。

## 完整本地原型展示

- [完整本地原型截图](assets/ui-full-local-prototype.png)
- [桌面布局截图](assets/ui-desktop.png)
- [紧凑布局截图](assets/ui-compact.png)

这些图片用于展示完整本地原型的界面设计，画面可包含 System probe、系统声来源、`system` / `mixed` 统计或不同的内存指标。相关捕获实现未包含在当前公开源码中，因此截图和视频是原型展示证据，不是当前 checkout 的逐像素运行结果或能力承诺。

## 未公开或依赖外部环境的能力

| 能力或资产 | 边界与原因 |
| --- | --- |
| System probe、WASAPI loopback、系统声与麦克风混合统计 | 完整本地原型中有展示，当前公开仓库没有可调用的系统声捕获链路；公开版仅承诺麦克风。 |
| 云端 ASR 与 Agent 推理 | 适配代码公开，但凭据、账户配额、Provider 服务和网络不随仓库提供。审阅者需使用自己的账户并接受相应服务条款。 |
| 开箱即用的联网搜索 | 仓库不提供搜索凭据、托管后端或生产 endpoint；搜索默认关闭，候选 query 也不能当作真实搜索结果。 |
| 真实会议数据、评测语料和运行日志 | 因隐私与数据授权边界不公开。仓库中的 `src/data/demoMeeting.ts` 是合成演示数据。 |
| 商业 SDK、企业系统连接器与生产配置 | 仓库不包含相关二进制、凭据、客户配置或企业环境，也不声称这些集成已经在公开版实现。 |
| 生产发布能力 | 仓库不包含代码签名证书、安全密钥存储、加密历史、多用户权限、合规审计、生产监控或 SLA。Windows 安装包配置存在，但当前发布候选未签名。 |

## 招聘方最短验证路径

环境：Node.js `20.19+` 或 `22.12+`，在仓库根目录运行：

```powershell
npm ci
npm run dev
```

按终端输出打开本地地址，然后：

1. 展开 **Add transcript segment**，输入一段普通会议文本并点击 **Add**。预期 Raw ASR segments 增加，Meeting Digest 明确提示需要 Agent Provider。
2. 再输入 `嗨助手 总结刚才内容`。预期界面捕获文本唤醒短语，并把问题带入 Copilot 输入区；未配置 Agent Provider 时不会伪造回答。
3. 点击 **Stop**。预期当前会议清空并在 Meeting history 中出现可查看、可删除的本地归档。

代码级验证：

```powershell
npm run lint
npm run build
cargo test --manifest-path src-tauri\Cargo.toml
```

完整麦克风链路需要 Windows 桌面依赖以及审阅者自己的 DashScope 和 Agent Provider 配置：

```powershell
npm run desktop:dev
```

配置成功后，预期 Start mic 可用，界面出现 interim/final 转写；final 片段进入时间线并触发外部文本模型生成纪要。该路径会产生第三方 API 调用和费用。

## 数据与隐私边界

- 麦克风音频由应用分帧后发送到配置的 DashScope 服务；本应用不把原始音频写入历史，但第三方如何处理数据取决于其服务条款与账户配置。
- 会议转写、纪要、问答和界面配置以明文保存在浏览器或 Tauri WebView 的 localStorage。它不适合直接处理机密会议。
- ASR Key 和 Agent Key 不写入 localStorage，但会存在于当前运行进程内存，并发送给对应 Provider；重启后需要重新填写。
- Agent 请求包含会议上下文。启用搜索后，query 还可能发送给自定义搜索 endpoint；基础脱敏不能替代隐私审查或企业 DLP。
- 公开仓库不包含真实会议转写、个人账号、API Key、浏览器配置或签名材料。

## 使用边界

本仓库是 **source-available portfolio**，仅供作品审阅与评估。公开可见不代表开源，也不自动授予复制、修改、再发布、托管、部署、派生作品或商业使用许可。权利边界以仓库根目录的 [LICENSE](../LICENSE) 为准；第三方依赖继续适用其各自许可证与 [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)。
