<script lang="ts">
  import { match } from 'ts-pattern';
  let { kind }: { kind: 'capture' | 'modules' | 'records' } = $props();
  const figure = $derived(
    match(kind)
      .with('capture', () => '1')
      .with('modules', () => '2')
      .with('records', () => '3')
      .exhaustive(),
  );
</script>

<div
  class="relative mb-9 h-80 max-[1100px]:h-65 max-md:mx-auto max-md:mb-6 max-md:h-75 max-md:max-w-97.5"
  data-ornament={kind}
  aria-hidden="true"
>
  <span class="absolute top-2 left-0 font-mono text-[10px] tracking-[1px] text-[#737c75]"
    >F. 0{figure}</span
  >
  <svg viewBox="0 0 360 290" fill="none" class="h-full w-full overflow-visible pt-6.75">
    {#if kind === 'capture'}
      <!-- Three preserved source records enter a single capture frame. -->
      <path d="M68 149v26l112 65 112-65v-26" stroke="#3d4740" stroke-linejoin="round" />
      <path
        d="m68 149 112-65 112 65-112 65-112-65Z"
        fill="#0b0d0c"
        stroke="#67736a"
        stroke-linejoin="round"
      />
      <path d="m92 149 88-51 88 51-88 51-88-51Z" stroke="#2f3b31" />
      <path d="m118 149 62-36 62 36-62 36-62-36Z" stroke="#3f5140" stroke-dasharray="2 5" />
      <path d="M180 214v26" stroke="#3d4740" />
      <g stroke="#879d81" opacity=".65">
        <path d="m127 169 106-61" />
        <circle cx="127" cy="169" r="2" fill="#879d81" stroke="none" />
      </g>
      {#each [{ x: 77, y: 55 }, { x: 142, y: 24 }, { x: 207, y: 55 }] as source, index}
        <path d={`M${source.x + 23} ${source.y + 65}v48`} stroke="#3c4c3d" stroke-dasharray="2 5" />
        <g class="motion-source" transform={`translate(${source.x} ${source.y})`}>
          <path d="M0 0 46 26v59L0 59Z" fill="#0b0d0c" stroke="#71816f" stroke-linejoin="round" />
          <path d="m0 0 6-3 46 26v59l-6 3M46 26l6-3" stroke="#38473a" stroke-linejoin="round" />
          <path d="m10 24 25 14m-25-4 25 14m-25-4 16 9" stroke="#465548" />
          <path d="m10 12 6 3v6l-6-3Z" fill={index === 1 ? '#91a884' : '#41553d'} />
        </g>
      {/each}
      <path d="m159 229 21 12 21-12" stroke="#829579" />
    {:else if kind === 'modules'}
      <path d="m96 151 84 44 88-47M180 92v103" stroke="#414c42" stroke-dasharray="3 4" />
      {#each [{ x: 180, y: 43, h: 92 }, { x: 102, y: 85, h: 100 }, { x: 261, y: 110, h: 87 }, { x: 180, y: 166, h: 55 }] as cube, i}
        <g class="motion-module" transform={`translate(${cube.x} ${cube.y})`}>
          <path
            d={`M-52 28 0 0 52 28v${cube.h}l-52 28-52-28Z`}
            fill="#0b0d0c"
            stroke="#535c56"
            stroke-linejoin="round"
          />
          <path d={`m-52 28 52 28 52-28M0 56v${cube.h}`} stroke="#2c342e" />
          <path
            d="m-10 26 10-5 10 5-10 5Z"
            fill={i === 3 ? '#8fa184' : '#253126'}
            stroke="#697e60"
          />
        </g>
      {/each}
    {:else}
      {#each Array.from({ length: 12 }, (_, i) => i) as sheet}
        <g class="motion-record" transform={`translate(${58 + sheet * 13} ${202 - sheet * 7})`}>
          <path
            d={`M0 0v${-22 - sheet * 10}q0-5 5-2l111 59q4 2 4 6v${22 + sheet * 10}q0 4-4 2L4 6Q0 4 0 0Z`}
            fill="#0b0d0c"
            stroke="#535c56"
          />
          <path d={`M6 ${-16 - sheet * 10} 113 ${41 - sheet * 10}`} stroke="#2c352e" />
        </g>
      {/each}
    {/if}
  </svg>
</div>
