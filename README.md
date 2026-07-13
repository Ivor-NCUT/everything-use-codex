# Everything Use Codex

**简体中文** | [English](README.en.md)

让每一个 App，都能使用用户本地的 Codex。

## 这个项目为什么值得存在

做一个 AI 产品，团队往往还没来得及验证真正有意思的交互，就要先搭一套重复的技术底座：模型接入、流式输出、工具调用、本地文件访问、任务中止、会话恢复、权限控制、用量管理。

Everything Use Codex 把这层执行能力放到用户自己的电脑上。App 负责收集 Context、描述任务和展示结果，具体执行交给用户已经安装并登录的 Codex。产品团队可以把时间花在用户怎样表达意图、结果怎样进入工作流，而不是反复维护 Agent 后端。

这会改变轻量 AI 产品的开发方式：

- 开发者无需为每个实验单独运营一套集中式推理后端。
- 用户可以继续使用已有的 Codex 环境、项目规则、工具和套餐内用量。
- 项目文件沿着本地执行链路流转；只有 Codex 或用户启用的工具需要时，相关内容才会离开本机。
- 一个很轻的 Context App，也可以检查仓库、修改文件、执行命令并延续会话。
- 不同前端可以共用一套开放协议，不必各自重写 Agent Loop。

我们希望它最终长成一套 Context App 生态。每个 App 提供一个专注的交互方式、一份任务结构、一组权限声明和一种结果视图；Everything Use Codex 为它们提供共同的执行契约。

Codex 的实际用量仍受用户的 OpenAI 套餐、额度和当期规则约束。本项目减少重复的 API 基础设施和产品方承担的集中推理成本，不承诺无限或永久免费的模型调用。

## 它提供什么

当前版本包含三层：

1. 一套带版本的 JSON-RPC 协议，定义任务、事件、错误、权限和生命周期。
2. 一个 Rust Runtime，负责启动本地 Codex CLI，并把 JSONL 输出转换成稳定事件。
3. 一个 TypeScript SDK，服务本机 Node.js 环境，包括 Next.js 服务端和 Electron 主进程。

每个宿主 App 启动自己的 Runtime 进程。App 退出时，Runtime 随之结束；整个过程不监听网络端口。

```text
本机 Next.js 服务端 / Electron / Tauri / 原生 App
                         │
                     对应语言 SDK
                         │ JSON-RPC 2.0 over stdio
                         ▼
            everything-codex-runtime
                         │ Codex JSONL
                         ▼
                    Codex CLI
```

进程边界能够隔离崩溃，也方便后续增加 Swift、.NET 和 Rust SDK。未来的 localhost Companion 仍可复用同一套任务服务与协议，只替换通信方式。

## 当前状态

项目目前处于 `0.1.0` 早期阶段，已经支持：

- macOS 和 Windows 构建目标
- Codex 可用性检查
- 新建和恢复 Codex thread
- 流式文本与命令事件
- 任务状态查询和中止
- `read-only`、`workspace-write`、`danger-full-access` 三档沙箱权限
- workspace 校验
- TypeScript 类型和 JSON Schema

第一版面向服务端进程运行在用户电脑上的 App。纯浏览器页面无法自行启动本地进程，需要本机服务端或桌面宿主承接 SDK。

## 仓库结构

```text
crates/codex-adapter    Codex 参数、进程启动与 JSONL 翻译
crates/runtime          stdio JSON-RPC Runtime 与任务生命周期
packages/protocol       TypeScript 协议类型与 JSON Schema
packages/sdk-typescript Node.js SDK
examples/node-basic     无 UI 的最小接入示例
```

## 从源码构建

环境要求：

- Rust stable
- Node.js 22 或更高版本
- pnpm 10
- 执行真实任务时，需要安装并登录 Codex CLI

```bash
pnpm install
cargo build --release -p everything-codex-runtime
pnpm build
```

Runtime 二进制文件位于：

```text
target/release/everything-codex-runtime
```

Windows 版本带有 `.exe` 后缀。

## TypeScript 快速开始

本地开发时，把编译后的 Runtime 路径传给 SDK：

```ts
import { createRuntime } from "@everything-use-codex/sdk";

const runtime = await createRuntime({
  runtimePath: "/absolute/path/to/everything-codex-runtime",
});

const codex = await runtime.checkCodex();
console.log(codex.version);

const task = await runtime.startTask({
  workspace: "/absolute/path/to/project",
  prompt: "检查这个项目并说明它的架构。",
  sandbox: "read-only",
});

task.onEvent((event) => {
  if (event.type === "text_delta") process.stdout.write(event.text);
  if (event.type === "completed") console.log("\n完成", event.thread_id);
});

await runtime.close();
```

打包 App 时，把对应平台的 Runtime 二进制文件放入应用资源，再将它的绝对路径传给 `createRuntime()`。

## 协议示例

stdin 和 stdout 每一行都是一条完整的 JSON-RPC 消息。

```json
{"jsonrpc":"2.0","id":1,"method":"runtime/initialize","params":{"protocolVersion":"1.0"}}
```

```json
{"jsonrpc":"2.0","id":2,"method":"task/start","params":{"prompt":"说明这个仓库的结构","workspace":"/project","sandbox":"read-only"}}
```

Runtime 通过通知发送事件：

```json
{"jsonrpc":"2.0","method":"task/event","params":{"taskId":"task-id","event":{"type":"text_delta","text":"我发现……"}}}
```

TypeScript 类型位于 [`packages/protocol`](packages/protocol)，机器可读的 Schema 位于 [`protocol.schema.json`](packages/protocol/schema/protocol.schema.json)。

## 安全模型

- Runtime 只通过 stdin 和 stdout 与父进程通信。
- 日志写入 stderr，确保协议输出始终可以被机器解析。
- workspace 必须是已经存在的绝对路径。
- 文件系统根目录和用户 home 根目录不能作为 workspace。
- SDK 默认使用 `workspace-write`。
- 宿主必须显式请求 `danger-full-access`。
- Runtime 不持久化 prompt、凭证和文件内容。

宿主 App 在请求敏感权限前，仍需向用户展示清晰的确认信息。

## 开发与验证

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
pnpm check
```

持续集成会在 macOS 和 Windows 上执行同一组检查。

## 路线图

- 为 macOS 和 Windows 打包经过签名的 Runtime 二进制文件
- 增加 Windows 进程终止的端到端测试夹具
- 将 TypeScript SDK 发布到 npm
- 增加 Tauri、Swift 和 .NET 宿主适配器
- 为浏览器 Context App 增加带鉴权的 localhost WebSocket transport

## 参与贡献

欢迎提交 Issue 和 Pull Request。修改协议时尽量保持向后兼容；增加新的 Codex 事件时补齐测试；通信层与任务执行层继续保持分离。

## License

MIT
