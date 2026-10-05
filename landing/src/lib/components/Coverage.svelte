<script lang="ts">
  import { ui } from '$lib/ui';

  // Measured counts of priced markets, 2026-10-05 (docs/v1.10-tokenized-stocks.md).
  const stats = [
    { value: '1,521', label: 'US equities', source: 'S&P 500, 400, 600 and Nasdaq 100' },
    { value: '308', label: 'Crypto spot', source: 'Top 500 by market cap' },
    { value: '197', label: 'Tokenized stocks', source: 'xStocks, Backpack, Robinhood, bStocks' },
    { value: '178', label: 'Perpetuals', source: 'Crypto perpetual futures' },
    { value: '37', label: 'FX pairs', source: 'Majors, Asia and beyond' },
    { value: '14', label: 'Commodities', source: 'Metals and energy' },
  ];
</script>

<section id="coverage" class="{ui.section} bg-[#10160f]" aria-labelledby="coverage-title">
  <div class="{ui.shell} text-center">
    <p class="reveal text-[11px] tracking-[0.12em] text-[#a0ad96]">MARKET COVERAGE</p>
    <p
      class="reveal mt-6 font-display text-[clamp(120px,18vw,240px)] leading-[0.9] tracking-[-0.06em]"
      aria-hidden="true"
    >
      2,200+
    </p>
    <h2 id="coverage-title" class="{ui.h2} reveal mt-6">
      Priced markets.<br /><span>One shape for all of them.</span>
    </h2>
    <dl class="reveal mt-20 grid grid-cols-6 border-t border-line max-md:mt-12 max-md:grid-cols-2">
      {#each stats as stat}<div
          class="flex flex-col border-r border-line px-4 pt-10 pb-2 last:border-r-0 max-md:border-b max-md:pb-8 max-md:even:border-r-0 max-md:nth-last-[-n+2]:border-b-0"
        >
          <!-- dt must precede dd; order-* restores number-first display. -->
          <dt class="order-2 mt-4 text-[15px] text-ink">{stat.label}</dt>
          <dd
            class="order-1 font-display text-[56px] leading-none tracking-[-0.04em] max-md:text-[44px]"
          >
            {stat.value}
          </dd>
          <dd class="order-3 mt-1 text-[13px] text-muted">{stat.source}</dd>
        </div>{/each}
    </dl>
  </div>
</section>
