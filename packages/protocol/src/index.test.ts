import { describe, expect, it } from "vitest";
import { isTaskEventNotification } from "./index.js";

describe("isTaskEventNotification", () => {
  it("accepts task events", () => {
    expect(
      isTaskEventNotification({
        jsonrpc: "2.0",
        method: "task/event",
        params: { taskId: "1", event: { type: "completed" } },
      }),
    ).toBe(true);
  });

  it("rejects responses", () => {
    expect(isTaskEventNotification({ jsonrpc: "2.0", id: 1, result: {} })).toBe(false);
  });
});

