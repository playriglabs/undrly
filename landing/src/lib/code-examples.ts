import { siCurl, siPython, siRust, siTypescript } from 'simple-icons';

export type Lang = 'shellscript' | 'typescript' | 'python' | 'rust' | 'json';
export type Example = {
  id: string;
  tab: string;
  name: string;
  /** 24×24 path: a filled simple-icons brand mark, or a stroked glyph when `stroke` is set. */
  icon: string;
  stroke?: boolean;
  lang: Lang;
  code: string;
};

// Plain HTTP against real routes (README "API"); MCP config from docs/v1.8-mcp.md §14.
export const examples: Example[] = [
  {
    id: 'curl',
    tab: 'cURL',
    name: 'cURL',
    icon: siCurl.path,
    lang: 'shellscript',
    code: `# One canonical quote, with its unit and freshness
curl -s -H "Authorization: Bearer $UNDRLY_API_KEY" \\
  "$UNDRLY_API_URL/v1/quote/BTC/USD"

# What a query refers to: resolved, ambiguous or not_found
curl -s -H "Authorization: Bearer $UNDRLY_API_KEY" \\
  "$UNDRLY_API_URL/v1/resolve?q=NVDA"

# A perpetual's mark, funding and open interest
curl -s -H "Authorization: Bearer $UNDRLY_API_KEY" \\
  "$UNDRLY_API_URL/v1/derivatives/BTC-PERP"

# Hourly OHLCV candles, oldest first
curl -s -H "Authorization: Bearer $UNDRLY_API_KEY" \\
  "$UNDRLY_API_URL/v1/candles/BTC/USD?interval=1h&limit=24"`,
  },
  {
    id: 'typescript',
    tab: 'TS',
    name: 'TypeScript',
    icon: siTypescript.path,
    lang: 'typescript',
    code: `const api = process.env.UNDRLY_API_URL;
const headers = { Authorization: \`Bearer \${process.env.UNDRLY_API_KEY}\` };

export async function getQuote(query: string) {
  const res = await fetch(\`\${api}/v1/quote/\${query}\`, { headers });
  if (!res.ok) throw new Error(\`undrly: \${res.status}\`);
  return res.json();
}

const quote = await getQuote('NVDA');

// Prices are exact decimal strings, never floats.
console.log(quote.price, quote.unit, quote.freshness);`,
  },
  {
    id: 'python',
    tab: 'PY',
    name: 'Python',
    icon: siPython.path,
    lang: 'python',
    code: `import os
from decimal import Decimal

import requests

api = os.environ["UNDRLY_API_URL"]
headers = {"Authorization": f"Bearer {os.environ['UNDRLY_API_KEY']}"}

quote = requests.get(f"{api}/v1/quote/EUR/USD", headers=headers).json()

# Exact decimal strings: parse with Decimal, not float.
price = Decimal(quote["price"])
print(price, quote["unit"], quote["freshness"])`,
  },
  {
    id: 'rust',
    tab: 'Rust',
    name: 'Rust',
    icon: siRust.path,
    lang: 'rust',
    code: `use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Deserialize)]
struct Quote {
    price: String,
    freshness: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = std::env::var("UNDRLY_API_URL")?;
    let key = std::env::var("UNDRLY_API_KEY")?;

    let quote: Quote = reqwest::Client::new()
        .get(format!("{api}/v1/quote/NVDA"))
        .bearer_auth(key)
        .send()
        .await?
        .json()
        .await?;

    // Exact decimal string: parse with Decimal, never f64.
    let price: Decimal = quote.price.parse()?;
    println!("{price} ({})", quote.freshness);
    Ok(())
}`,
  },
  {
    id: 'mcp',
    tab: 'Agent (MCP)',
    name: 'your agent',
    // A bot: head, antenna, eyes and ears.
    icon: 'M4 8h16v12H4zM12 8V4.5M12 3.25v.01M9 13v2M15 13v2M2 12.5v3M22 12.5v3',
    stroke: true,
    lang: 'json',
    code: `{
  "mcpServers": {
    "undrly": {
      "command": "bun",
      "args": ["run", "typescript/apps/mcp/src/stdio.ts"],
      "env": {
        "UNDRLY_API_URL": "https://api.undrly.xyz",
        "UNDRLY_API_KEY": "<your api key>"
      }
    }
  }
}`,
  },
];
