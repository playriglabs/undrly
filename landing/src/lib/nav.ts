import { links } from '$lib/links';

export type MenuKey = 'solutions' | 'resources' | 'developers';
export type Item = { label: string; href: string; menu?: MenuKey };
export type Entry = { title: string; copy: string; href: string };
export type Menu = {
  title: string;
  entries: Entry[];
  feature: { heading: string; title: string; copy: string; cta: string; href: string };
};

export const items: Item[] = [
  { label: 'Solutions', href: '#capabilities', menu: 'solutions' },
  { label: 'Resources', href: links.docs, menu: 'resources' },
  { label: 'Pricing', href: '#pricing' },
  { label: 'Developers', href: '#developers', menu: 'developers' },
];
export const menus: Record<MenuKey, Menu> = {
  solutions: {
    title: 'Solutions',
    entries: [
      {
        title: 'Resolve',
        copy: 'Any ticker, ISIN or on-chain address to one canonical identity.',
        href: '#capabilities',
      },
      {
        title: 'Graph',
        copy: 'What it derives from, tracks, settles in and trades on.',
        href: '#capabilities',
      },
      {
        title: 'Prices',
        copy: 'One canonical quote per market, always with its unit.',
        href: '#capabilities',
      },
      {
        title: 'Market data',
        copy: 'Candles, reference rates and perpetual funding.',
        href: '#capabilities',
      },
    ],
    feature: {
      heading: 'Built for agents',
      title: 'MCP server',
      copy: 'Read-only tools over the same API routes.',
      cta: 'Read the guide',
      href: links.mcp,
    },
  },
  resources: {
    title: 'Resources',
    entries: [
      { title: 'Documentation', copy: 'Guides and reference for every route.', href: links.docs },
      { title: 'Blog', copy: 'Product updates and notes on market data.', href: links.blog },
      { title: 'MCP', copy: 'Connect agents to undrly over MCP.', href: links.mcp },
      { title: 'Support', copy: 'Report an issue or ask a question.', href: links.contact },
    ],
    feature: {
      heading: 'Get started',
      title: 'Quickstart',
      copy: 'Run undrly locally and make your first query.',
      cta: 'Open quickstart',
      href: links.quickstart,
    },
  },
  developers: {
    title: 'Developers',
    entries: [
      {
        title: 'API',
        copy: 'Read-only GET routes for quotes, graphs and history.',
        href: links.api,
      },
      { title: 'SDK', copy: 'Typed TypeScript contracts for every response.', href: links.sdk },
      { title: 'MCP server', copy: 'The same routes as tools for agents.', href: links.mcp },
    ],
    feature: {
      heading: 'API reference',
      title: 'GET /v1/quote/{query}',
      copy: 'One canonical quote for a market, with its unit and freshness.',
      cta: 'See all routes',
      href: links.api,
    },
  },
};
export const menuKeys = Object.keys(menus) as MenuKey[];
