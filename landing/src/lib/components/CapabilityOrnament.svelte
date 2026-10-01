<script lang="ts">
  // Single-stroke glyphs, one per capability card.
  let { kind }: { kind: 'resolve' | 'graph' | 'prices' | 'market' | 'agents' } = $props();
</script>

<svg
  class="h-auto w-[150px]"
  viewBox="0 0 160 120"
  fill="none"
  stroke="currentColor"
  stroke-width="1.5"
  aria-hidden="true"
>
  {#if kind === 'resolve'}
    <!-- Look up any identifier; land on one confirmed identity. -->
    <circle cx="70" cy="52" r="36" />
    <path d="M96 78l32 32" />
    <path d="M54 52l11 11 21-22" />
  {:else if kind === 'graph'}
    <!-- One instrument linked to its related markets. -->
    <path d="M80 60 30 22M80 60l50-38M80 60 30 98M80 60l50 38" />
    <circle cx="80" cy="60" r="14" fill="var(--color-card)" />
    {#each [[30, 22], [130, 22], [30, 98], [130, 98]] as [cx, cy]}
      <circle {cx} {cy} r="8" fill="var(--color-card)" />
    {/each}
  {:else if kind === 'prices'}
    <!-- A coin: every price carries its currency unit. -->
    <circle cx="80" cy="60" r="50" />
    <path
      d="M99 44C96 36 89 33 80 33C69 33 62 39 62 47C62 56 71 58 80 60C89 62 98 64 98 73C98 81 91 87 80 87C70 87 62 83 60 75M80 24v12M80 84v12"
    />
  {:else if kind === 'agents'}
    <!-- An agent's prompt: a chat bubble holding a command line. -->
    <path d="M24 20H136V84H64L44 102V84H24Z" stroke-linejoin="round" />
    <path d="M54 40l14 12-14 12M78 64h26" stroke-linecap="round" stroke-linejoin="round" />
  {:else}
    <!-- Candles: open, close, and range. -->
    {#each [{ x: 34, top: 30, body: [48, 84], low: 100 }, { x: 80, top: 14, body: [26, 70], low: 88 }, { x: 126, top: 38, body: [54, 94], low: 108 }] as c}
      <path d="M{c.x} {c.top}V{c.body[0]}M{c.x} {c.body[1]}V{c.low}" />
      <rect x={c.x - 11} y={c.body[0]} width="22" height={c.body[1] - c.body[0]} />
    {/each}
  {/if}
</svg>
