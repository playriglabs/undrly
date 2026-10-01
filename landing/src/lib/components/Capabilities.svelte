<script lang="ts">
  import { ui } from '$lib/ui';
  import Arrow from './Arrow.svelte';
  import CapabilityOrnament from './CapabilityOrnament.svelte';

  const cards = [
    {
      title: 'Resolve',
      kind: 'resolve' as const,
      copy: 'Any ticker, ISIN or on-chain address to one canonical identity.',
    },
    {
      title: 'Graph',
      kind: 'graph' as const,
      copy: 'What it derives from, tracks, settles in and trades on. Every link sourced.',
    },
    {
      title: 'Prices',
      kind: 'prices' as const,
      copy: 'One canonical quote per market, always with its unit.',
    },
    {
      title: 'Market data',
      kind: 'market' as const,
      copy: 'Candles, reference rates and perpetual funding from the same query.',
    },
    {
      title: 'Agents',
      kind: 'agents' as const,
      copy: 'The same queries as read-only MCP tools, so agents read markets like your app.',
    },
  ];

  let track: HTMLUListElement;
  let atStart = $state(true);
  let atEnd = $state(false);

  function update() {
    atStart = track.scrollLeft <= 1;
    atEnd = track.scrollLeft + track.clientWidth >= track.scrollWidth - 1;
  }
  // One card per press: card width plus the gap.
  function step(direction: 1 | -1) {
    const card = track.firstElementChild as HTMLElement | null;
    if (!card) return;
    track.scrollBy({ left: direction * (card.offsetWidth + 16), behavior: 'smooth' });
  }
  const navButton =
    'flex size-11 items-center justify-center border border-line-strong text-ink transition-[background,opacity] duration-[180ms] hover:bg-card disabled:pointer-events-none disabled:opacity-35';
</script>

<section
  id="capabilities"
  class="{ui.section} border-y border-[#252e22] bg-[#10160f]"
  aria-labelledby="capabilities-title"
>
  <div class={ui.shell}>
    <div class="{ui.sectionHeading} reveal flex items-end justify-between gap-10">
      <h2 id="capabilities-title" class={ui.h2}>
        Everything a market needs<br /><span>to know about itself.</span>
      </h2>
      <div class="flex shrink-0 gap-2 max-md:hidden">
        <button
          class={navButton}
          aria-label="Previous capabilities"
          aria-controls="capabilities-track"
          disabled={atStart}
          onclick={() => step(-1)}
        >
          <span class="flex rotate-180"><Arrow /></span>
        </button>
        <button
          class={navButton}
          aria-label="Next capabilities"
          aria-controls="capabilities-track"
          disabled={atEnd}
          onclick={() => step(1)}
        >
          <Arrow />
        </button>
      </div>
    </div>
    <!-- Cards keep their grid width; the ones past the edge scroll in from the right. -->
    <ul
      id="capabilities-track"
      class="reveal flex snap-x snap-mandatory gap-4 overflow-x-auto overscroll-x-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      bind:this={track}
      onscroll={update}
    >
      {#each cards as card}<li
          class="flex min-h-105 shrink-0 basis-[calc((100%-3rem)/3.5)] snap-start max-lg:basis-[calc((100%-1rem)/2)] flex-col bg-card p-9 text-center max-md:min-h-85 max-md:basis-[85%] max-md:px-6 max-md:py-7"
        >
          <div class="flex flex-1 items-center justify-center pb-8 text-[#e3e8df]">
            <CapabilityOrnament kind={card.kind} />
          </div>
          <h3 class="text-2xl tracking-[-0.02em]">{card.title}</h3>
          <!-- Three reserved lines keep every title on the same baseline. -->
          <p class="mx-auto mt-2.5 min-h-[4.8em] text-[15px] leading-[1.6] text-muted">
            {card.copy}
          </p>
        </li>{/each}
    </ul>
  </div>
</section>
