# Everything Use Codex

Give every app a local Codex runtime.

## Why this project matters

AI products spend a surprising amount of work rebuilding the same backend: model access, streaming, tool execution, local files, task cancellation, session recovery, permissions, and usage management. Product teams repeat that work before they can test the interaction that made their idea interesting.

Everything Use Codex moves that shared execution layer onto the user's computer. An app collects context, describes a task, and hands it to the Codex installation the user already has. The product team can focus on how people express intent and how results fit their workflow.

This changes the economics of small AI products:

- Developers do not have to operate a centralized inference backend for every experiment.
- Users can reuse their existing Codex environment, project instructions, tools, and included plan usage.
- Sensitive project files stay in the local execution path unless Codex or an enabled tool sends them elsewhere.
- A lightweight context app can still perform real work: inspect repositories, edit files, run commands, and continue a thread.
- Many frontends can share one open protocol instead of inventing their own agent loop.

The long-term goal is an ecosystem of context apps. Each app contributes a focused interaction, a task schema, a permission request, and a result view. Everything Use Codex supplies the execution contract underneath them.

Codex usage remains subject to the user's OpenAI plan, limits, and current terms. This project reduces duplicated API infrastructure and product-side inference cost; it does not promise unlimited or permanently free model usage.

## What it provides

Everything Use Codex currently ships three layers:

1. A versioned JSON-RPC protocol for tasks, events, errors, permissions, and lifecycle.
2. A Rust runtime that starts the local Codex CLI and turns its JSONL stream into stable events.
3. A TypeScript SDK for local Node.js environments, including Next.js servers and Electron main processes.

Each host app starts its own runtime process. The runtime exits with the host and listens on no network port.

```text
Next.js local server / Electron / Tauri / native app
                         │
                  language SDK
                         │ JSON-RPC 2.0 over stdio
                         ▼
            everything-codex-runtime
                         │ Codex JSONL
                         ▼
                    Codex CLI
```

The process boundary keeps crashes isolated and leaves room for future Swift, .NET, and Rust SDKs. A later localhost companion can reuse the same task service and protocol with a different transport.

## Current status

The project is an early `0.1.0` foundation. It supports:

- macOS and Windows build targets
- Codex availability checks
- new and resumed Codex threads
- streaming text and command events
- task status and cancellation
- read-only, workspace-write, and full-access sandbox requests
- workspace validation
- TypeScript types and JSON Schema

The first release targets apps whose server process runs on the user's machine. A browser-only deployment cannot start local processes by itself.

## Repository layout

```text
crates/codex-adapter    Codex arguments, process startup, JSONL translation
crates/runtime          stdio JSON-RPC runtime and task lifecycle
packages/protocol       TypeScript protocol types and JSON Schema
packages/sdk-typescript Node.js SDK
examples/node-basic     minimal integration without a UI
```

## Build from source

Requirements:

- Rust stable
- Node.js 22 or later
- pnpm 10
- Codex CLI installed and signed in for real task execution

```bash
pnpm install
cargo build --release -p everything-codex-runtime
pnpm build
```

The runtime binary will be at:

```text
target/release/everything-codex-runtime
```

On Windows it has an `.exe` suffix.

## TypeScript quick start

During local development, point the SDK at the compiled runtime:

```ts
import { createRuntime } from "@everything-use-codex/sdk";

const runtime = await createRuntime({
  runtimePath: "/absolute/path/to/everything-codex-runtime",
});

const codex = await runtime.checkCodex();
console.log(codex.version);

const task = await runtime.startTask({
  workspace: "/absolute/path/to/project",
  prompt: "Inspect this project and explain its architecture.",
  sandbox: "read-only",
});

task.onEvent((event) => {
  if (event.type === "text_delta") process.stdout.write(event.text);
  if (event.type === "completed") console.log("\nDone", event.thread_id);
});

await runtime.close();
```

For packaged apps, ship the matching runtime binary as an application resource and pass its absolute path to `createRuntime()`.

## Protocol example

Every stdin and stdout line is one JSON-RPC message.

```json
{"jsonrpc":"2.0","id":1,"method":"runtime/initialize","params":{"protocolVersion":"1.0"}}
```

```json
{"jsonrpc":"2.0","id":2,"method":"task/start","params":{"prompt":"Explain this repository","workspace":"/project","sandbox":"read-only"}}
```

Runtime events arrive as notifications:

```json
{"jsonrpc":"2.0","method":"task/event","params":{"taskId":"task-id","event":{"type":"text_delta","text":"I found..."}}}
```

The TypeScript definitions live in [`packages/protocol`](packages/protocol), and the machine-readable schema is [`protocol.schema.json`](packages/protocol/schema/protocol.schema.json).

## Security model

- The runtime communicates only with its parent process over stdin and stdout.
- Logs go to stderr, keeping protocol output machine-readable.
- Workspaces must be existing absolute directories.
- Filesystem roots and the user's home root are rejected as workspaces.
- The SDK defaults to `workspace-write`.
- `danger-full-access` requires an explicit host request.
- Prompts, credentials, and file contents are not persisted by this runtime.

The host app remains responsible for showing an appropriate user confirmation before requesting sensitive capabilities.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
pnpm check
```

CI runs the same checks on macOS and Windows.

## Roadmap

- Package signed runtime binaries for macOS and Windows
- Add end-to-end fixtures for process termination on Windows
- Publish the TypeScript SDK to npm
- Add Tauri, Swift, and .NET host adapters
- Add an authenticated localhost WebSocket transport for browser-based context apps

## Contributing

Issues and pull requests are welcome. Keep protocol changes backward-compatible whenever possible, add tests for new Codex events, and keep transport code separate from task execution.

## License

MIT
