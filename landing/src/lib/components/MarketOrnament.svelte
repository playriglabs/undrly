<script lang="ts">
  // Single-stroke glyphs, one per asset class; same grid and stroke as CapabilityOrnament.
  let { kind }: { kind: 'crypto' | 'commodities' | 'fx' | 'equities' } = $props();
</script>

<svg
  class="h-auto w-[150px]"
  viewBox="0 0 160 120"
  fill="none"
  stroke="currentColor"
  stroke-width="1.5"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if kind === 'crypto'}
    <!-- Two coins: Ether behind, Bitcoin in front. -->
    <circle cx="106" cy="50" r="36" />
    <circle cx="106" cy="50" r="30" />
    <path d="M108 28 120 51 108 58 96 51ZM96 51l12-6 12 6M96 55l12 17 12-17-12 7Z" />
    <circle cx="56" cy="70" r="36" fill="var(--color-card)" />
    <circle cx="56" cy="70" r="30" />
    <path
      d="M44 54V86M44 54H56C63 54 66 57 66 62S63 70 56 70H44M44 70H58C65 70 68 73 68 78S65 86 58 86H44M48 50v4M54 50v4M48 86v4M54 86v4"
    />
  {:else if kind === 'commodities'}
    <!-- Two bars stacked (gold, silver) and a drop of oil. -->
    <path d="M12 100 24 76H80L92 100Z" />
    <path d="M24 76 32 66H72L80 76" />
    <path d="M32 66 39 46H65L72 66" />
    <path d="M39 46 45 37H59L65 46" />
    <path d="M124 24C124 24 102 54 102 72A22 22 0 0 0 146 72C146 54 124 24 124 24Z" />
  {:else if kind === 'fx'}
    <!-- A dollar note and a yen coin. -->
    <rect x="14" y="30" width="92" height="56" />
    <rect x="20" y="36" width="80" height="44" />
    <path
      d="M70 48C68 43 64 42 60 42C54 42 50 45 50 50S55 56 60 58 70 61 70 66 66 74 60 74C55 74 51 72 50 67M60 37v5M60 74v5"
    />
    <circle cx="116" cy="78" r="28" fill="var(--color-card)" />
    <circle cx="116" cy="78" r="22" />
    <path d="M106 64l10 14 10-14M116 78v14M107 80h18M107 86h18" />
  {:else}
    <!-- The exchange: a columned hall. -->
    <path d="M28 42 80 16l52 26ZM28 48H132M24 98H136M20 104H140" />
    {#each [36, 62, 88, 114] as x}
      <rect {x} y="54" width="10" height="38" />
    {/each}
  {/if}
</svg>
