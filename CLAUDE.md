# Everything Use Codex

本仓库提供可嵌入宿主应用的本地 Codex Runtime、JSON-RPC 协议与 TypeScript SDK。

## 技术栈

- Rust：Runtime、任务生命周期、Codex CLI 进程与 JSONL 翻译。
- TypeScript：公开协议类型、JSON Schema 和 Node.js SDK。
- pnpm workspace + Cargo workspace：统一管理构建和测试。

## 目录

- `crates/codex-adapter`：Codex CLI 参数、进程启动和事件翻译。
- `crates/runtime`：JSON-RPC stdio transport 与任务服务。
- `packages/protocol`：稳定的公开协议类型和 Schema。
- `packages/sdk-typescript`：供 Next.js 本机服务端、Electron 和 Node.js 使用的 SDK。
- `examples/node-basic`：最小无 UI 接入示例。
- `docs/superpowers/specs`：经过确认的设计规格。

## 架构约束

- stdout 只输出 NDJSON 协议消息，日志写 stderr。
- transport 不包含 Codex 业务逻辑。
- SDK 不解析 Codex JSONL。
- 默认 sandbox 为 `workspace-write`；高权限必须显式请求。
- 结构或接口改变时同步更新本文件、README 和设计规格。

## 验证

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
pnpm check
```

