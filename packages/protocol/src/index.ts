export const PROTOCOL_VERSION = "1.0" as const;

export type Sandbox = "read-only" | "workspace-write" | "danger-full-access";
export type TaskStatus =
  | "starting"
  | "running"
  | "stopping"
  | "completed"
  | "failed"
  | "interrupted";

export type TaskEvent =
  | { type: "thread_started"; thread_id: string }
  | { type: "text_delta"; text: string }
  | { type: "tool_started"; id: string; name: string; input: unknown }
  | { type: "tool_completed"; id: string; output: string; is_error: boolean }
  | {
      type: "usage";
      input_tokens?: number;
      output_tokens?: number;
      cached_input_tokens?: number;
      reasoning_output_tokens?: number;
    }
  | { type: "completed"; thread_id?: string }
  | { type: "failed"; message: string }
  | { type: "interrupted" };

export interface StartTaskInput {
  taskId?: string;
  prompt: string;
  workspace: string;
  threadId?: string;
  model?: string;
  sandbox?: Sandbox;
  images?: string[];
}

export interface RuntimeInfo {
  runtimeVersion: string;
  protocolVersion: string;
  platform: string;
  capabilities: string[];
}

export interface CodexCheck {
  available: boolean;
  binary: string;
  version: string;
}

export interface RpcRequest<T = unknown> {
  jsonrpc: "2.0";
  id: number;
  method: string;
  params: T;
}

export interface RpcSuccess<T = unknown> {
  jsonrpc: "2.0";
  id: number;
  result: T;
}

export interface RpcFailure {
  jsonrpc: "2.0";
  id: number | null;
  error: {
    code: number;
    message: string;
    data?: { code?: string; [key: string]: unknown };
  };
}

export interface TaskEventNotification {
  jsonrpc: "2.0";
  method: "task/event";
  params: { taskId: string; event: TaskEvent };
}

export type RpcIncoming = RpcSuccess | RpcFailure | TaskEventNotification;

export function isTaskEventNotification(value: unknown): value is TaskEventNotification {
  if (!value || typeof value !== "object") return false;
  const message = value as Record<string, unknown>;
  return message.jsonrpc === "2.0" && message.method === "task/event";
}

