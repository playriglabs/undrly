import { createFileRoute } from "@tanstack/react-router";
import { Panel } from "../../components/ui";

export const Route = createFileRoute("/_app/api-keys")({
  head: () => ({ meta: [{ title: "API keys — Undrly" }] }),
  component: ApiKeys,
});

const th =
  "px-5 py-3 text-left font-mono text-[10.5px] font-normal tracking-[0.1em] text-faint uppercase";

/**
 * UI only: the API does not issue or check keys yet. Nothing here pretends to
 * create one; the table stays empty until key issuing exists.
 */
function ApiKeys() {
  return (
    // The layout bleeds to the right edge (for Explore's table); this page keeps its gutter.
    <div className="pr-8 max-md:pr-4">
      <div className="flex items-end justify-between gap-8 max-md:flex-col max-md:items-start max-md:gap-4">
        <div>
          <h1 className="text-[29px] leading-none font-sans tracking-[-0.03em] max-md:text-[34px]">
            API keys
          </h1>
          <p className="mt-3 max-w-140 text-[15px] leading-[1.6] text-muted">
            Keys authenticate requests to the Undrly API and the MCP server. Send one as a bearer
            token on every request.
          </p>
        </div>
        <button
          type="button"
          disabled
          title="Key issuing arrives with API authentication"
          className="border border-[#dbe4d3] bg-[#dbe4d3] px-4 py-2.5 text-[14px] text-[#1a2317] opacity-50"
        >
          Create key
        </button>
      </div>

      <p className="mt-8 border border-dashed border-line-strong px-5 py-4 text-[14px] text-muted">
        Key issuing isn’t available yet. The API is read-only and open during the preview; keys will
        be required once authentication ships.
      </p>

      <div className="mt-6 border border-line">
        <table className="w-full border-collapse">
          <thead className="border-b border-line bg-panel">
            <tr>
              <th className={th}>Name</th>
              <th className={th}>Key</th>
              <th className={th}>Created</th>
              <th className={th}>Last used</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td colSpan={4} className="px-5 py-14 text-center text-[14px] text-faint">
                No keys yet.
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <Panel title="Usage" className="mt-6">
        <pre className="overflow-x-auto px-5 py-4 font-mono text-[13px] leading-[1.9] text-[#dfe3dc]">
          {`curl -s -H "Authorization: Bearer $UNDRLY_API_KEY" \\
  "https://api.undrly.xyz/v1/quote/BTC/USD"`}
        </pre>
      </Panel>
    </div>
  );
}
