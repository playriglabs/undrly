<script lang="ts">
  import { ui } from '$lib/ui';
  import MarketOrnament from './MarketOrnament.svelte';

  // Examples are markets in the universe (docs/v1.1-universe.md, docs/v1.2-fx.md, docs/v1.3-market-data.md).
  const markets = [
    {
      title: 'Crypto',
      kind: 'crypto' as const,
      copy: 'Spot assets and perpetual futures, with funding and open interest.',
      examples: ['BTC', 'ETH', 'BTC perp'],
    },
    {
      title: 'Commodities',
      kind: 'commodities' as const,
      copy: 'Metals and energy benchmarks, each with its unit of measure.',
      examples: ['Gold', 'Silver', 'WTI', 'Brent'],
    },
    {
      title: 'Forex',
      kind: 'fx' as const,
      copy: 'Major pairs, Southeast Asian, MENA currencies, orientation preserved.',
      examples: ['EUR/USD', 'USD/SGD', 'USD/JPY'],
    },
    {
      title: 'Equities',
      kind: 'equities' as const,
      copy: 'S&P 500 constituents with sessions, calendars and corporate actions.',
      examples: ['NVDA', 'AAPL', 'MSFT'],
    },
  ];
</script>

<section id="markets" class={ui.section} aria-labelledby="markets-title">
  <div class={ui.shell}>
    <div class="{ui.sectionHeading} {ui.headingRow} reveal">
      <h2 id="markets-title" class={ui.h2}>
        Every asset class.<br /><span>Read the same way.</span>
      </h2>
      <p class={ui.sectionCopy}>
        One identity, quote and unit model across crypto, commodities, forex and equities.
      </p>
    </div>
    <div class="reveal grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4">
      {#each markets as market}<article
          class="flex min-h-105 flex-col bg-card p-9 text-center max-md:min-h-85 max-md:px-6 max-md:py-7"
        >
          <div class="flex flex-1 items-center justify-center pb-8 text-[#e3e8df]">
            <MarketOrnament kind={market.kind} />
          </div>
          <h3 class="text-2xl tracking-[-0.02em]">{market.title}</h3>
          <!-- Three reserved lines keep every title on the same baseline. -->
          <p class="mt-2.5 min-h-[4.8em] text-[15px] leading-[1.6] text-muted">{market.copy}</p>
          <ul class="mt-5 flex flex-wrap justify-center gap-1.5">
            {#each market.examples as example}<li
                class="border border-line-strong px-2 py-1 text-[11px] text-[#a1a6a0]"
              >
                {example}
              </li>{/each}
          </ul>
        </article>{/each}
    </div>
  </div>
</section>
