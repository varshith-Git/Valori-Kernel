import { NextRequest } from "next/server";
import { pm } from "@/lib/server/process-manager";

export const dynamic = "force-dynamic";

export async function GET(req: NextRequest) {
  const nodeId = Number(req.nextUrl.searchParams.get("nodeId") ?? "1");
  const enc = new TextEncoder();

  const stream = new ReadableStream({
    start(controller) {
      let cursor = 0;

      // Send buffered logs from the start
      const initial = pm.getLogs(nodeId, 0);
      for (const line of initial.lines) {
        controller.enqueue(enc.encode(`data: ${JSON.stringify(line)}\n\n`));
      }
      cursor = initial.cursor;

      const interval = setInterval(() => {
        const { lines, cursor: next } = pm.getLogs(nodeId, cursor);
        if (lines.length > 0) {
          for (const line of lines) {
            controller.enqueue(enc.encode(`data: ${JSON.stringify(line)}\n\n`));
          }
          cursor = next;
        }
        // heartbeat every ~5s keeps connection alive through proxies
      }, 200);

      // heartbeat
      const heartbeat = setInterval(() => {
        controller.enqueue(enc.encode(`: heartbeat\n\n`));
      }, 5000);

      req.signal.addEventListener("abort", () => {
        clearInterval(interval);
        clearInterval(heartbeat);
        controller.close();
      });
    },
  });

  return new Response(stream, {
    headers: {
      "Content-Type":  "text/event-stream",
      "Cache-Control": "no-cache, no-transform",
      "Connection":    "keep-alive",
      "X-Accel-Buffering": "no",
    },
  });
}
