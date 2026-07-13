import { createRuntime } from "@everything-use-codex/sdk";

const args = process.argv.slice(2).filter((value, index) => value !== "--" || index > 0);
const workspace = args[0] ?? process.cwd();
const prompt = args.slice(1).join(" ") || "请简要说明这个目录里有什么。不要修改文件。";
const runtime = await createRuntime();

try {
  console.log(await runtime.checkCodex());
  const task = await runtime.startTask({ prompt, workspace, sandbox: "read-only" });
  await new Promise<void>((resolve, reject) => {
    task.onEvent((event) => {
      console.log(JSON.stringify(event));
      if (event.type === "completed" || event.type === "interrupted") resolve();
      if (event.type === "failed") reject(new Error(event.message));
    });
  });
  await runtime.close();
} catch (error) {
  console.error(error);
  await runtime.close();
  process.exitCode = 1;
}
