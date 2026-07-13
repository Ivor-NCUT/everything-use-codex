import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import {
  PROTOCOL_VERSION,
  isTaskEventNotification,
  type CodexCheck,
  type RpcFailure,
  type RuntimeInfo,
  type StartTaskInput,
  type TaskEvent,
  type TaskStatus,
} from "@everything-use-codex/protocol";

export * from "@everything-use-codex/protocol";

export class RuntimeError extends Error {
  constructor(
    message: string,
    readonly code: string,
    readonly rpcCode?: number,
  ) {
    super(message);
    this.name = "RuntimeError";
  }
}

export interface CreateRuntimeOptions {
  runtimePath?: string;
  codexPath?: string;
  requestTimeoutMs?: number;
}

type EventListener = (event: TaskEvent) => void;
type Pending = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: NodeJS.Timeout;
};

export class TaskHandle {
  private readonly listeners = new Set<EventListener>();
  private readonly history: TaskEvent[] = [];

  constructor(
    readonly taskId: string,
    private readonly runtime: CodexRuntime,
  ) {}

  onEvent(listener: EventListener): () => void {
    this.listeners.add(listener);
    for (const event of this.history) listener(event);
    return () => this.listeners.delete(listener);
  }

  dispatch(event: TaskEvent): void {
    this.history.push(event);
    for (const listener of this.listeners) listener(event);
  }

  async status(): Promise<TaskStatus> {
    return this.runtime.taskStatus(this.taskId);
  }

  async stop(): Promise<void> {
    await this.runtime.stopTask(this.taskId);
  }
}

export class CodexRuntime {
  private nextId = 1;
  private readonly pending = new Map<number, Pending>();
  private readonly tasks = new Map<string, TaskHandle>();
  private readonly earlyEvents = new Map<string, TaskEvent[]>();
  private closed = false;

  private constructor(
    private readonly child: ChildProcessWithoutNullStreams,
    private readonly requestTimeoutMs: number,
  ) {
    const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
    lines.on("line", (line) => this.receive(line));
    child.on("exit", (code, signal) => {
      this.closed = true;
      this.rejectAll(new RuntimeError(`Runtime exited (${code ?? signal ?? "unknown"})`, "RUNTIME_EXITED"));
    });
    child.stderr.on("data", (chunk) => process.stderr.write(`[everything-codex] ${chunk}`));
  }

  static async create(options: CreateRuntimeOptions = {}): Promise<CodexRuntime> {
    const runtimePath = options.runtimePath ?? process.env.EVERYTHING_CODEX_RUNTIME ?? "everything-codex-runtime";
    const env = { ...process.env };
    if (options.codexPath) env.EVERYTHING_CODEX_BIN = options.codexPath;
    const child = spawn(runtimePath, [], { stdio: ["pipe", "pipe", "pipe"], env });
    await new Promise<void>((resolve, reject) => {
      child.once("spawn", resolve);
      child.once("error", reject);
    });
    const runtime = new CodexRuntime(child, options.requestTimeoutMs ?? 30_000);
    await runtime.request<RuntimeInfo>("runtime/initialize", { protocolVersion: PROTOCOL_VERSION });
    return runtime;
  }

  info(): Promise<RuntimeInfo> {
    return this.request("runtime/initialize", { protocolVersion: PROTOCOL_VERSION });
  }

  checkCodex(): Promise<CodexCheck> {
    return this.request("codex/check", {});
  }

  async startTask(input: StartTaskInput): Promise<TaskHandle> {
    const result = await this.request<{ taskId: string }>("task/start", {
      ...input,
      sandbox: input.sandbox ?? "workspace-write",
      images: input.images ?? [],
    });
    const handle = new TaskHandle(result.taskId, this);
    this.tasks.set(result.taskId, handle);
    for (const event of this.earlyEvents.get(result.taskId) ?? []) handle.dispatch(event);
    this.earlyEvents.delete(result.taskId);
    return handle;
  }

  async taskStatus(taskId: string): Promise<TaskStatus> {
    const result = await this.request<{ status: TaskStatus }>("task/status", { taskId });
    return result.status;
  }

  async stopTask(taskId: string): Promise<void> {
    await this.request("task/stop", { taskId });
  }

  async close(): Promise<void> {
    if (this.closed) return;
    try {
      await this.request("runtime/shutdown", {});
    } finally {
      this.closed = true;
      this.child.stdin.end();
    }
  }

  private request<T>(method: string, params: unknown): Promise<T> {
    if (this.closed) return Promise.reject(new RuntimeError("Runtime is closed", "RUNTIME_CLOSED"));
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new RuntimeError(`Request timed out: ${method}`, "REQUEST_TIMEOUT"));
      }, this.requestTimeoutMs);
      this.pending.set(id, { resolve: resolve as (value: unknown) => void, reject, timer });
      this.child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  }

  private receive(line: string): void {
    let message: unknown;
    try {
      message = JSON.parse(line);
    } catch {
      this.rejectAll(new RuntimeError("Runtime emitted invalid JSON", "INVALID_RUNTIME_OUTPUT"));
      return;
    }
    if (isTaskEventNotification(message)) {
      const task = this.tasks.get(message.params.taskId);
      if (task) {
        task.dispatch(message.params.event);
      } else {
        const events = this.earlyEvents.get(message.params.taskId) ?? [];
        events.push(message.params.event);
        this.earlyEvents.set(message.params.taskId, events);
      }
      return;
    }
    if (!message || typeof message !== "object" || !("id" in message)) return;
    const response = message as { id: number; result?: unknown; error?: RpcFailure["error"] };
    const pending = this.pending.get(response.id);
    if (!pending) return;
    this.pending.delete(response.id);
    clearTimeout(pending.timer);
    if (response.error) {
      pending.reject(new RuntimeError(response.error.message, response.error.data?.code ?? "RUNTIME_ERROR", response.error.code));
    } else {
      pending.resolve(response.result);
    }
  }

  private rejectAll(error: Error): void {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
  }
}

export function createRuntime(options?: CreateRuntimeOptions): Promise<CodexRuntime> {
  return CodexRuntime.create(options);
}
