# Everything Use Codex Runtime 设计规格

## 1. 项目定位

Everything Use Codex 是一个可嵌入任意本地应用的 Codex 运行时与通信协议。应用开发者只负责设计 Context 获取方式和产品交互；Runtime 负责调用用户本机已有的 Codex，管理任务、会话、权限和流式事件。

第一阶段不提供前端，不建设云端推理服务，也不绑定 Next.js、Electron 或 Tauri。首要交付物是可被宿主应用随进程启动和退出的本地 Runtime，以及面向 Node.js/Next.js 服务端的 TypeScript SDK。

## 2. 已确认的架构决策

- 每个宿主 App 内嵌一份 Runtime，Runtime 随宿主启动和退出。
- Runtime 是独立二进制，不是宿主进程内的动态库。
- 宿主通过 JSON-RPC 2.0 over stdio 与 Runtime 通信。
- 第一版 TypeScript SDK 面向 Node.js 环境，因此可用于本机 Next.js 服务端和 Electron 主进程。
- 浏览器页面不直接启动 Runtime；它通过本机 Next.js 服务端或桌面宿主间接使用 SDK。
- Runtime 首先支持 macOS 和 Windows。
- Codex CLI 是第一版唯一 Agent 后端。
- 协议、Runtime 和 Codex Adapter 相互隔离，为后续 localhost WebSocket transport 保留复用边界。

## 3. 范围

### 3.1 第一版必须支持

1. 查询 Runtime 版本和协议版本。
2. 检测 Codex CLI 是否存在并可执行。
3. 在指定 workspace 启动 Codex 任务。
4. 将 Codex JSONL 输出转换为稳定的协议事件。
5. 流式发送文本、工具调用、工具结果、用量、完成和错误事件。
6. 停止正在运行的任务。
7. 保存并返回 Codex thread ID，允许宿主显式恢复会话。
8. 校验 workspace 存在且是目录，拒绝空路径和明显过宽的路径。
9. 支持 Runtime 退出时清理所有子进程。
10. 提供 TypeScript SDK，封装二进制启动、请求关联、事件订阅和退出清理。
11. 在 macOS 和 Windows CI 上完成类型检查、单元测试和构建。

### 3.2 第一版明确不做

- 产品 UI 或示例设计系统。
- 用户账号、云端服务或远程中继。
- localhost HTTP/WebSocket 服务。
- 全局常驻 Runtime。
- Claude、Gemini 或其他 Agent Adapter。
- 自动更新与安装器 UI。
- Linux 发布包。
- 任意插件系统。
- Runtime 自行存储长期聊天历史。

## 4. 仓库结构

仓库采用 pnpm workspace 与 Cargo workspace：

```text
everything-use-codex/
├── packages/
│   ├── protocol/          # TypeScript 协议类型、校验与版本
│   └── sdk-typescript/    # Node.js/Next.js/Electron SDK
├── crates/
│   ├── runtime/           # JSON-RPC stdio 服务、任务注册与生命周期
│   └── codex-adapter/     # Codex CLI argv、进程与 JSONL 翻译
├── examples/
│   └── node-basic/        # 无 UI 的最小接入示例
├── docs/
│   └── superpowers/specs/
├── .github/workflows/
├── Cargo.toml
├── package.json
└── README.md
```

Rust 侧持有进程与任务状态；TypeScript SDK 不重复实现 Codex 行为。

## 5. 协议设计

### 5.1 传输约束

- stdin 与 stdout 使用 UTF-8 NDJSON，每行一个完整 JSON-RPC 消息。
- stdout 只允许协议消息；诊断日志全部写入 stderr。
- 每个请求必须带 `jsonrpc: "2.0"`、唯一 `id`、`method` 和 `params`。
- 通知不带 `id`。
- 第一版协议版本为 `1.0`，通过 `runtime/initialize` 协商。

### 5.2 方法

#### `runtime/initialize`

宿主启动后必须首先调用。返回 Runtime 版本、协议版本、平台和能力。

#### `codex/check`

检查 Codex 二进制是否存在并可执行。第一版不通过读取凭证文件判断登录状态；真实可用性以受控命令执行结果为准。

#### `task/start`

参数包含：

- `taskId`：可选；缺省时由 Runtime 生成。
- `prompt`：非空字符串。
- `workspace`：绝对路径。
- `threadId`：可选；存在时恢复对应 Codex thread。
- `model`：可选。
- `sandbox`：`read-only | workspace-write | danger-full-access`。
- `images`：可选的本地图片绝对路径数组。

返回 `taskId`。运行过程通过 `task/event` 通知发送。

#### `task/stop`

按 `taskId` 停止任务。重复停止已结束任务返回稳定的 `TASK_NOT_RUNNING` 错误，不制造新的状态分支。

#### `task/status`

返回单个任务的 `starting | running | stopping | completed | failed | interrupted` 状态。

#### `runtime/shutdown`

停止所有子进程，等待有限宽限期后退出 Runtime。

### 5.3 统一事件

`task/event` 的 `event` 使用判别联合：

- `thread_started`
- `text_delta`
- `tool_started`
- `tool_completed`
- `usage`
- `completed`
- `failed`
- `interrupted`

协议只暴露稳定语义，不直接泄漏 Codex CLI 的原始事件结构。未知 Codex 事件记录到 stderr，但不会导致任务失败。

### 5.4 错误码

- `PROTOCOL_VERSION_UNSUPPORTED`
- `INVALID_PARAMS`
- `RUNTIME_NOT_INITIALIZED`
- `CODEX_NOT_FOUND`
- `CODEX_UNAVAILABLE`
- `WORKSPACE_INVALID`
- `WORKSPACE_TOO_BROAD`
- `TASK_ALREADY_EXISTS`
- `TASK_NOT_FOUND`
- `TASK_NOT_RUNNING`
- `TASK_SPAWN_FAILED`
- `TASK_FAILED`
- `INTERNAL_ERROR`

错误响应包含稳定 `code`、用户可读 `message` 和可选 `data`，不把本机敏感环境变量或完整命令行泄漏给宿主。

## 6. Codex Adapter

### 6.1 新任务

Adapter 通过 stdin 传入 prompt，并启动类似命令：

```text
codex exec --json --sandbox <mode> --skip-git-repo-check -C <workspace> -
```

第一版不强制覆盖用户的全局 Codex 配置，不默认加入 `approval_policy="never"`，也不默认使用 `danger-full-access`。SDK 默认 sandbox 为 `workspace-write`；宿主必须显式请求更高权限。

### 6.2 恢复任务

当传入 `threadId` 时使用：

```text
codex exec --sandbox <mode> --skip-git-repo-check -C <workspace> resume --json <threadId> -
```

### 6.3 停止和退出

- macOS：先发送温和终止信号，超过宽限期后强制终止。
- Windows：使用 Tokio 进程能力终止子进程；实现必须验证不会遗留直接 Codex 子进程。
- Runtime 收到 EOF、关闭请求或自身终止信号时清理所有活跃任务。

## 7. TypeScript SDK

SDK 提供最小接口：

```ts
const runtime = await createRuntime();
const task = await runtime.startTask({
  prompt,
  workspace,
  sandbox: "workspace-write",
});

task.onEvent((event) => {});
await task.stop();
await runtime.close();
```

SDK 职责仅包括：

- 定位或接收 Runtime 二进制路径。
- 启动子进程。
- JSON-RPC 请求与响应关联。
- 事件分发。
- Runtime 意外退出时拒绝所有未完成请求并结束任务订阅。
- 宿主退出时关闭 Runtime。

SDK 不解析 Codex JSONL，不决定 workspace 权限，也不保存 thread 历史。

## 8. 安全边界

- Runtime 只接受来自其父进程 stdio 的请求，不监听端口。
- workspace 必须是绝对路径、真实存在的目录，并经过规范化。
- 第一版拒绝文件系统根目录和用户 home 根目录作为 workspace。
- 默认 sandbox 为 `workspace-write`。
- `danger-full-access` 必须由宿主显式传入；协议保留该事实，便于宿主展示确认界面。
- prompt、环境变量、文件内容和 Codex 凭证不写入 Runtime 持久存储。
- stdout 严格保持机器可读，避免日志注入破坏协议帧。

## 9. 数据流

1. 宿主通过 SDK 启动 Runtime。
2. SDK 调用 `runtime/initialize`。
3. 宿主调用 `task/start`。
4. Runtime 校验参数、workspace 和任务 ID。
5. Runtime 通过 Codex Adapter 启动子进程并写入 prompt。
6. Adapter 按行解析 Codex stdout JSONL，转换为统一事件。
7. Runtime 通过 `task/event` 向 SDK 推送事件。
8. 终态事件发出后，Runtime 清理进程句柄并保留有限的内存状态供 `task/status` 查询。
9. 宿主关闭时，SDK 调用 `runtime/shutdown`；异常退出则依赖 EOF 和进程清理兜底。

## 10. 测试策略

### 10.1 Rust 单元测试

- Codex argv 新任务与恢复任务。
- JSONL 到统一事件的翻译。
- 未知事件、畸形 JSON、非零退出码和无终态退出。
- workspace 规范化和过宽路径拒绝。
- 任务状态转换。

### 10.2 Runtime 集成测试

使用可控 fake Codex 可执行文件验证：

- initialize、check、start、status、stop、shutdown。
- stdout 只含合法 NDJSON。
- stderr 日志不会污染协议。
- Runtime 退出后子进程被清理。

### 10.3 TypeScript SDK 测试

- 请求 ID 关联。
- 并发任务事件按 task ID 分发。
- Runtime 意外退出。
- 超时和主动 close。
- 错误码透传。

### 10.4 CI

GitHub Actions 在 `macos-latest` 与 `windows-latest` 上运行：

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test --workspace`
- `pnpm typecheck`
- `pnpm test`
- `pnpm build`

## 11. 后续路线 B

路线 B 不改变任务协议和 Codex Adapter。它增加一个独立 Companion 外壳：

- 将 JSON-RPC transport 从 stdio 扩展到 localhost WebSocket。
- 增加设备配对、Origin 白名单、短期 token、nonce、防重放和本地网络授权。
- Runtime 核心仍复用同一任务服务。

因此第一版禁止把业务方法与 stdin/stdout 读写耦合在同一模块中；transport 只负责收发 JSON-RPC，任务服务负责实际行为。

## 12. README 内容顺序

公开 README 必须按以下顺序组织：

1. 项目愿景和战略价值。
2. 为什么让产品使用用户自己的 Codex。
3. 它为 AI 产品开发者消除了哪些重复后端工作。
4. 架构和协议如何实现。
5. 安装、快速开始和 API。
6. 安全模型、平台支持、路线图和贡献方式。

README 不承诺“永久零 token cost”。它应准确表述为复用用户已有的 Codex 订阅额度与本地能力，减少产品方集中承担 API 推理成本；实际用量受用户计划和 OpenAI 当期规则约束。

## 13. 完成标准

- Node.js 示例能在 macOS 和 Windows 上通过 SDK 启动 Runtime。
- Runtime 能用 fake Codex 完成全链路自动化测试。
- 在已安装并登录 Codex 的机器上，示例能执行真实任务并收到完成事件。
- 所有公开协议类型有 TypeScript 定义和 JSON Schema。
- README 先说明战略价值，再说明实现。
- GitHub 仓库创建并推送主分支，CI 配置可见。
