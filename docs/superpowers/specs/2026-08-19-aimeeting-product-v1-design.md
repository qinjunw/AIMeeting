# AIMeeting Windows 产品化 V1 设计规格

**日期：** 2026-08-19
**状态：** 已确认
**目标平台：** Windows 10/11
**后续平台：** Android（本阶段不实现）

## 1. 产品目标

AIMeeting V1 是一款中文优先的桌面会议记录工具。用户打开应用后，可以选择麦克风和系统声音作为输入，开始、暂停、结束录音；应用在云端服务可用时实时转写，在服务不可用时仍可靠保存录音，并允许会后重新转写和生成简体中文会议纪要。

V1 需要同时满足以下目标：

- 看起来和使用起来像桌面应用，不像网页或开发调试面板。
- 麦克风与 Windows 系统声音默认同时录制，也可以在开始前分别关闭。
- 两路声音经过独立采集和统一混音，形成一份用户可播放的本地录音。
- 实时转写失败、网络断开或 Key 错误时，录音继续且不丢失。
- 转写和会议纪要都可以独立重试，不阻塞录音生命周期。
- 会议、音频、转写、纪要都持久化，除非用户主动永久删除。
- 支持回收站、恢复和清空回收站，不自动清理数据。
- 为 Android 平台、远端 Meeting Room 和统一 AI Gateway 留下明确接口，但不提前实现产品功能。

## 2. 非目标

以下能力不进入 V1：

- Android 客户端实现。
- 远端会议房间、匹配码、成员同步和服务端部署。
- 单独捕获某个应用的声音。
- 扬声器场景下的完整声学回声消除（AEC）。
- 说话人分离、声纹识别和自动标注具体姓名。
- 实时翻译、多语言纪要模板。
- 唤醒词、副驾助手、联网搜索和人工添加转写片段。
- 内置或自动下载本地 ASR 模型。
- 强制安装、自动更新和公开发行所需的正式代码签名。

## 3. 技术路线决策

### 3.1 方案比较

#### 方案 A：保留 Tauri，录音核心迁移到 Rust（采用）

- React/TypeScript 负责桌面界面和状态展示。
- Rust 负责音频设备、系统回环采集、混音、编码、文件写入、云端连接和 SQLite。
- 使用 Tauri command/event 作为窄接口，不再通过 WebView 主线程搬运大量 Base64 音频。

优点是复用现有项目，发布体积较小，Windows 音频能力和长录音稳定性更可控，也能继续沿用 Tauri 2 的移动端能力边界。

#### 方案 B：继续使用 Web Audio 采集

改造量较小，但 Windows 系统声音捕获、长时间运行、崩溃恢复、主线程压力和 WebView 差异都更难稳定控制。它适合浏览器原型，不适合作为产品主音频引擎。

#### 方案 C：重写为 .NET MAUI/WinUI

Windows 原生集成更直接，但需要重写现有 React/Tauri 代码，Android 音频能力仍需平台适配，当前阶段成本明显高于收益。

### 3.2 最终选择

采用方案 A。Tauri 只是桌面壳和前后端桥接，用户不会接触浏览器地址；生产包内嵌前端资源，运行时不启动 Vite 服务。界面保留 Windows 原生窗口边框和系统行为，内容区域按桌面工具设计。

## 4. 用户体验

### 4.1 主窗口

主窗口由三部分组成：

1. 左侧历史列表：会议标题、时间、时长、处理状态，支持搜索、进入回收站和新建会议。
2. 中间内容区：只保留“会议纪要”和“完整转写”两个页签。
3. 底部固定录音栏：麦克风、系统声音、开始/暂停/继续/结束、计时和简洁状态。

录音源默认全部开启。V1 中录音源在开始录音前或暂停时调整，正在录音时锁定，避免设备切换造成时间轴和文件损坏。

### 4.2 状态表达

界面不展示模型协议、分片、PCM、WebSocket 或内部队列等工程术语，只显示用户需要采取行动的状态：

- `正在录音`
- `已暂停`
- `正在转写`
- `转写暂不可用，录音仍在保存`
- `正在整理纪要`
- `可重新转写`
- `已完成`

错误使用内容区顶部的单行状态条和对应操作按钮呈现，不使用连续弹窗。

### 4.3 设置窗口

设置使用居中的桌面模态窗口，分为：

- 录音：麦克风设备、系统输出设备、默认录音源、存储位置。
- 语音转文字：实时 ASR Provider、文件 ASR Provider、Base URL、Model、API Key、连接测试。
- 会议纪要：文字模型 Provider、Base URL、Model、API Key、连接测试。
- 通用：简体中文输出、数据目录、关于。

API Key 输入框不回显完整密钥，密钥不再写入 `localStorage`。

### 4.4 便携式发布

首个测试发布采用 ZIP 便携包：用户解压后双击 `AIMeeting.exe`，不写注册表、不创建卸载项。业务数据写入 `%LOCALAPPDATA%\AIMeeting`，不会因替换程序目录而丢失。

便携式不等于免除安全验证：未签名 EXE 可能触发 Windows SmartScreen。小范围测试可以接受提示；面向公众分发前仍应购买代码签名证书并签署 EXE。若目标机器缺少 WebView2 Runtime，启动检查需要给出明确安装提示。

安装版作为后续发布选项，届时再加入卸载、快捷方式、协议注册和自动更新。

## 5. 核心架构

```text
React Desktop UI
        |
        | Tauri commands + typed events
        v
Meeting Application Service (Rust)
        |
        +-- Recording Coordinator
        |     +-- Microphone Capture Adapter
        |     +-- System Loopback Capture Adapter
        |     +-- Audio Mixer / Preprocessor
        |     +-- Crash-safe Audio Recorder
        |
        +-- Transcription Coordinator
        |     +-- LiveTranscriptionGateway
        |     +-- FileTranscriptionGateway
        |
        +-- Minutes Coordinator
        |     +-- MinutesGateway
        |
        +-- Meeting Repository (SQLite + local files)
        |
        +-- RoomGateway (interface only)
```

### 5.1 模块边界

前端不直接访问云端 Provider，不持有音频处理状态机，也不负责业务数据持久化。Rust 后端成为单一事实来源，前端只发送用户意图并订阅状态快照。

建议的 Rust 模块：

```text
src-tauri/src/
  app/                 # 用例编排和状态机
  audio/               # 采集、混音、重采样、预处理、编码
  domain/              # Meeting Record、Recording Run、Job 等类型
  gateways/
    live_asr/          # 实时 ASR 适配器
    file_asr/          # 文件转写适配器
    minutes/           # 纪要模型适配器
    room/              # RoomGateway 接口和 unavailable 实现
  persistence/         # SQLite、文件布局、迁移、回收站
  commands/            # Tauri command/event DTO
```

建议的前端模块：

```text
src/
  app/                 # 应用壳、路由、错误边界
  features/meetings/   # 历史、详情、录音栏、回收站
  features/settings/   # 设置模态窗口
  components/          # 小型桌面组件
  bridge/              # Tauri commands/events 类型封装
```

## 6. 音频链路

### 6.1 Windows 采集

- 麦克风通过 Windows 音频输入设备采集。
- 系统声音通过 WASAPI loopback 捕获默认或指定输出设备的总混音。
- 先进行一个限时技术验证，对比 CPAL 的 Windows loopback 支持和直接使用 `windows` crate 调用 WASAPI。
- 如果 CPAL 在目标 Windows 版本、蓝牙耳机或设备热插拔测试中不稳定，则系统声音适配器回退到直接 WASAPI，领域层和 UI 不变。

### 6.2 混音和预处理

两个采集适配器输出带单调时钟时间戳的浮点帧。`AudioMixer` 将它们统一为 48 kHz、mono、f32 内部格式，通过有界缓冲吸收设备时钟轻微漂移，再进行：

- 分路增益控制。
- 防削波 limiter。
- 基础静音检测和电平统计。
- 可替换的 `AudioPreprocessor` 接口。

V1 不实现完整 AEC。产品说明和首次使用提示建议佩戴耳机；扬声器场景只做基础防削波，不承诺消除系统声音从麦克风再次进入造成的回声。

### 6.3 双路输出

混音后的同一条规范音频流分成两个消费者：

1. 录音分支：编码为 Ogg Opus 单声道语音文件并持续落盘。
2. ASR 分支：按 Provider 要求重采样为 16 kHz PCM16 小帧，送入实时转写。

录音分支优先级高于 ASR 分支。ASR 队列满、网络阻塞或 Provider 断开时，只降级转写，不允许反压录音文件写入。

### 6.4 暂停、恢复和崩溃安全

用户看到的是一个统一录音资产。内部允许按 `Recording Run` 写入可恢复的临时片段和日志；结束会议后再无损封装为一个 `recording.ogg`。这样既满足单文件体验，也避免应用异常退出时整段容器不可恢复。

- 暂停：停止向录音和 ASR 分支写入，刷新当前临时片段，Meeting Record 保持打开。
- 继续：创建新的 Recording Run，沿用同一个 Meeting Record。
- 结束：禁止新帧，刷新文件，等待有上限的 ASR 收尾，创建最终纪要任务，然后将会议转为可浏览状态。
- 异常退出：下次启动扫描未结束记录和临时片段，恢复为“上次录音意外中断”，允许播放已保存部分、继续处理或结束归档。

## 7. 转写和纪要

### 7.1 AI Gateway 能力接口

统一 `AiGateway` 是应用编排入口，但不把不同模型强行伪装成同一种协议。内部保留三个能力接口：

- `LiveTranscriptionGateway`：持续音频帧，产生 interim/final 事件。
- `FileTranscriptionGateway`：接受本地音频资产，产生完整带时间戳转写。
- `MinutesGateway`：接受转写快照和模板，输出简体中文结构化纪要。

一个 Provider 可以实现一个或多个能力。设置页分别选择实时 ASR、文件 ASR 和纪要模型，避免把 Whisper、Paraformer 和文字大模型混为一项配置。

### 7.2 Provider 策略

- 现有 DashScope Paraformer 实时链路先迁移为 `LiveTranscriptionGateway` 适配器。
- 文件转写优先支持 OpenAI-compatible transcription/Whisper 类接口；具体首个 Provider 在接口稳定后接入。
- 纪要模型继续支持 OpenAI-compatible chat 接口，但请求移动到 Rust 并增加超时、取消、重试和响应校验。
- API Key 使用 Windows Credential Manager 或 Tauri Stronghold 类安全存储，SQLite 只保存非敏感 Provider 配置和密钥引用。

### 7.3 自动降级和重新转写

开始录音不等待 ASR 握手成功。实时 ASR 可用时显示低延迟字幕；不可用时立即标记为“待转写”，继续保存音频。

结束后，如果实时转写不完整或失败，用户可以点击“重新转写”。文件转写创建持久化任务，不覆盖原始音频；成功后生成新的 transcript revision，再触发纪要重建。

### 7.4 纪要算法

原始 final transcript 首先持久化，纪要在独立队列中生成：

- 每个 Meeting Record 同时最多运行一个纪要任务。
- 达到时间或文本阈值时合并触发，避免每句话调用一次模型。
- 每个任务携带 transcript revision；较旧结果不得覆盖较新结果。
- 停止会议后执行一次最终整理。
- 纪要失败不影响录音和完整转写，可单独重试。
- 输出固定为简体中文，保留专有名词、数字、结论、行动项和未决问题，不凭空补充事实。

V1 建议的默认纪要结构为：

- 会议概览
- 关键讨论
- 决策结论
- 行动项
- 未决问题

## 8. 数据模型和持久化

### 8.1 SQLite 表

- `meeting_records`：标题、状态、开始/结束时间、转写状态、纪要状态、`deleted_at`。
- `recording_runs`：所属会议、序号、开始/结束、源配置、临时文件状态。
- `recording_assets`：逻辑音频、最终路径、格式、时长、字节数、校验信息。
- `transcript_segments`：revision、final 文本、Provider 时间戳、置信度、状态。
- `meeting_minutes`：revision、结构化内容、模型信息、生成状态。
- `processing_jobs`：任务类型、状态、尝试次数、错误摘要、输入 revision。
- `provider_profiles`：非敏感配置和安全密钥引用。
- `app_settings`：设备选择、存储位置、界面偏好。

### 8.2 文件布局

```text
%LOCALAPPDATA%\AIMeeting\
  aimeeting.db
  meetings\<meeting-id>\
    recording.ogg
    recovery\
  trash\<meeting-id>\
```

数据库保存索引和状态，音频保存为普通文件。写入采用临时文件、刷新、原子重命名和校验，禁止继续依赖 `localStorage` 作为业务数据仓库。

### 8.3 回收站

删除 Meeting Record 时设置 `deleted_at` 并将其目录移动到 `trash`。恢复操作撤销这一过程。只有“永久删除”或“清空回收站”才删除音频、转写、纪要和数据库行；回收站不自动过期，并显示占用空间。

### 8.4 旧数据迁移

首次启动新版本时检测旧 `localStorage` 会议数据，提供一次性导入。旧记录没有音频，标记为“旧版文本记录”；导入成功后不立即删除旧数据，直到用户确认。

## 9. 生命周期状态机

```text
preparing -> recording <-> paused -> stopping -> processing -> ready
                  |                         |
                  +---- interrupted <------+

ready/paused/interrupted -> trashed -> ready 或 permanently_deleted
```

转写和纪要状态不复用会议主状态，各自使用 `idle/running/degraded/failed/ready`。这样“正在录音但转写失败”是合法且可恢复的状态，不需要把整个会议标记为失败。

每个异步结果都携带 `meeting_id`、`run_id` 和 revision。UI 切换到新会议后，旧会议晚到的结果只能写回旧会议，不能污染当前会议。

## 10. 远端 Room Gateway 预留

本阶段只定义不可用实现和领域 DTO：

- `create_room()`
- `join_room(match_code)`
- `leave_room(room_id)`
- `subscribe_room_events(room_id)`

匹配码生成、校验、成员身份、音频上传和同步协议都由未来服务器负责。V1 不在界面显示入口，也不在本地伪造房间功能。

`Meeting Room` 与 `Meeting Record` 必须保持独立：加入房间不自动等于录音，退出房间也不自动删除本地记录。

## 11. Android 可移植边界

Android 本阶段不创建工程，但以下能力必须通过接口隔离：

- `AudioCaptureAdapter`
- `AudioEncoder`
- `AppDataPaths`
- `SecretStore`
- `LiveTranscriptionGateway`
- `FileTranscriptionGateway`
- `MinutesGateway`
- `RoomGateway`

领域状态机、数据库模型、Provider DTO 和纪要策略不得依赖 Windows 类型。Windows 系统声音录制属于平台能力，Android 版本不承诺提供完全相同的系统音频捕获语义。

## 12. 分阶段交付

### Phase 0：基线和音频技术验证

- 冻结并记录当前功能基线。
- 用真实设备验证麦克风、WASAPI loopback、双路混音、蓝牙耳机、设备断开和 30 分钟运行。
- 在 CPAL 与直接 WASAPI 之间做出有测试证据的选择。

### Phase 1：持久化录音核心

- 引入领域状态机、SQLite、文件布局和崩溃恢复。
- 实现双源采集、混音、Opus 持久化、暂停/恢复/结束。
- 录音完成前不重做现有界面。

### Phase 2：Provider 和处理任务

- 将现有实时 Paraformer 迁入 Gateway。
- 接入文件转写、持久任务、重试、超时和 revision 保护。
- 将纪要请求迁入 Rust，完成简体中文结构化纪要。

### Phase 3：桌面产品界面

- 拆分当前大型 `App.tsx` 和 CSS。
- 落地主窗口、历史、两页签、底部录音栏、设置和回收站。
- 移除或隐藏开发控制、解释性文字、唤醒助手和手工 segment。

### Phase 4：可靠性和便携发布

- 完成 30 分钟稳定性、断网、错误 Key、异常退出、快速停止再开始测试。
- 构建便携 ZIP，验证干净 Windows 机器启动、WebView2 检查和数据目录。
- 记录安装版和代码签名的后续发布流程，但不强制进入首个测试包。

## 13. 验收标准

以下条件全部满足才视为 Windows V1 可供外部测试：

1. 麦克风、系统声音、双源混音三种模式都能生成可播放录音。
2. 双源默认开启，设备选择和暂停后调整不会导致崩溃或串会。
3. 连续录音 30 分钟，内存无持续失控增长，音频时长误差和可听断裂在可接受范围内。
4. ASR Key 错误、断网、Provider 超时均不影响本地录音，结束后可以重新转写。
5. 暂停后继续属于同一 Meeting Record；结束后立即开始新会议不会接收上一会议的晚到结果。
6. 应用异常退出后能发现并恢复未完成会议的已落盘音频。
7. final transcript 持久化后才进入纪要任务；旧 revision 不得覆盖新纪要。
8. 删除进入回收站，恢复后数据完整，永久删除会同时清除文件和数据库记录。
9. 便携包解压后可一键启动，运行不依赖 Vite、Node.js 或 Rust 开发环境。
10. 生产界面不展示开发探针、原始协议日志或长篇功能说明。

## 14. 主要风险和约束

- Windows 系统音频捕获在不同驱动、蓝牙设备和独占模式下差异较大，必须先做 Phase 0 实机验证。
- 麦克风与系统输出来自不同硬件时钟，混音器必须处理缓冲和漂移，不能只按到达顺序拼帧。
- 耳机优先只是 V1 产品边界，不等于代码可以忽略未来 AEC；`AudioPreprocessor` 必须可替换。
- 便携包不写注册表，但仍受 WebView2 和 SmartScreen 影响；公开发布前需要签名和干净机器验证。
- Android 的音频权限、后台录制和系统音频能力与 Windows 不同，可移植的是领域与 Gateway，不是 Windows 采集实现。

## 15. 参考资料

- [Tauri Architecture](https://v2.tauri.app/concept/architecture/)
- [Tauri Distribution](https://v2.tauri.app/distribute/)
- [Microsoft WASAPI Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- [MDN ScriptProcessorNode deprecation](https://developer.mozilla.org/en-US/docs/Web/API/ScriptProcessorNode/audioprocess_event)
- [DashScope ASR models](https://help.aliyun.com/zh/model-studio/asr-model/)
