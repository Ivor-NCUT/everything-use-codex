import { describe, expect, it } from "vitest";
import { RuntimeError } from "./index.js";

describe("RuntimeError", () => {
  it("keeps stable protocol details", () => {
    const error = new RuntimeError("missing", "TASK_NOT_FOUND", -32000);
    expect(error.code).toBe("TASK_NOT_FOUND");
    expect(error.rpcCode).toBe(-32000);
  });
});

