/**
 * Test helper: an MCP client connected in memory to the Undrly MCP server,
 * served exactly as `src/stdio.ts` serves it (`serveStdio` with one factory),
 * in either protocol era.
 */
import { Client, InMemoryTransport } from "@modelcontextprotocol/client";
import { serveStdio } from "@modelcontextprotocol/server/stdio";
import type { UndrlyApi } from "../src/api.ts";
import { createUndrlyMcpServer, type ServerOptions } from "../src/server.ts";

export type Era = "legacy" | "modern";

export async function connect(api: UndrlyApi, era: Era = "legacy", options: ServerOptions = {}) {
  const [serverSide, clientSide] = InMemoryTransport.createLinkedPair();
  const handle = serveStdio(() => createUndrlyMcpServer(api, options), { transport: serverSide });
  const client = new Client(
    { name: "undrly-mcp-test", version: "0" },
    era === "modern" ? { versionNegotiation: { mode: { pin: "2026-07-28" } } } : {},
  );
  await client.connect(clientSide);
  const call = async (name: string, args: Record<string, unknown> = {}) => {
    const r = await client.callTool({ name, arguments: args });
    return {
      isError: r.isError === true,
      // biome-ignore lint/suspicious/noExplicitAny: test access to structured JSON
      data: r.structuredContent as any,
      text: (r.content as { type: string; text: string }[])[0]?.text ?? "",
    };
  };
  const close = async () => {
    await client.close();
    await handle.close();
  };
  return { client, call, close };
}
