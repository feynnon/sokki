// Drives the app's WebView on a connected Android device or emulator
// (debug builds only), so tests need no hands:
//   bun scripts/cdp.ts eval '<js expression>'      → prints the JSON result
//   bun scripts/cdp.ts add <audio file> <seconds>  → adds a recording without the mic
// Needs `adb` and one device (set ANDROID_SERIAL to pick one).
import { $ } from "bun";

async function wsUrl(): Promise<string> {
  const sockets = (await $`adb shell cat /proc/net/unix`.text())
    .split("\n")
    .map((l) => l.match(/@(webview_devtools_remote_\d+)/)?.[1])
    .filter(Boolean) as string[];
  if (!sockets.length) throw new Error("No debuggable WebView: is the app running?");
  await $`adb forward tcp:9333 localabstract:${sockets[sockets.length - 1]}`.quiet();
  const pages = await (await fetch("http://127.0.0.1:9333/json")).json();
  const page = pages.find((p: { type: string }) => p.type === "page") ?? pages[0];
  return page.webSocketDebuggerUrl;
}

async function evaluate(expression: string): Promise<unknown> {
  const ws = new WebSocket(await wsUrl());
  await new Promise((r) => ws.addEventListener("open", r, { once: true }));
  const reply = new Promise<any>((resolve) =>
    ws.addEventListener("message", (e) => resolve(JSON.parse(String(e.data))), { once: true }),
  );
  ws.send(
    JSON.stringify({
      id: 1,
      method: "Runtime.evaluate",
      params: { expression, awaitPromise: true, returnByValue: true },
    }),
  );
  const r = await reply;
  ws.close();
  if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails));
  return r.result?.result?.value;
}

const [cmd, ...args] = process.argv.slice(2);
if (cmd === "eval") {
  console.log(JSON.stringify(await evaluate(args.join(" ")), null, 2));
} else if (cmd === "add") {
  const [file, seconds] = args;
  const b64 = Buffer.from(await Bun.file(file).arrayBuffer()).toString("base64");
  const type = file.endsWith(".webm") ? "audio/webm" : "audio/wav";
  console.log(
    await evaluate(`window.__verbatim.add(${JSON.stringify(b64)}, "${type}", ${Number(seconds) || 0})`),
  );
} else if (cmd === "dictate") {
  // Dictates a 16 kHz mono 16-bit WAV in real time through the app's
  // dictation pipeline; prints seconds from stop to final text, and the text.
  // Optional: minimum and maximum piece length in seconds.
  const b64 = Buffer.from(await Bun.file(args[0]).arrayBuffer()).toString("base64");
  const lengths = { minS: Number(args[1]) || undefined, maxS: Number(args[2]) || undefined };
  const r = await evaluate(`window.__dictate(${JSON.stringify(b64)}, ${JSON.stringify(lengths)})`);
  console.log(JSON.stringify(r));
  console.log(await evaluate("window.__dictateText()"));
} else {
  console.error("usage: cdp.ts eval <js> | add <file> <seconds> | dictate <wav> [min_s max_s]");
  process.exit(2);
}
