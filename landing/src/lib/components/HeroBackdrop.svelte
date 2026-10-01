<script lang="ts">
  import clsx from 'clsx';
  import { onMount } from 'svelte';

  // Two stepped grid clusters (top-right, bottom-left). Rows list how many
  // cells each row has, counted from the cluster's corner, so the inner edge
  // reads as a staircase.
  const cell = 80;
  const clusters = [
    { corner: 'top-right', rows: [7, 8, 6, 5, 6, 5, 6, 3] },
    { corner: 'bottom-left', rows: [6, 7, 5, 6, 4, 5, 2] },
  ] as const;
  const width = (rows: readonly number[]) => Math.max(...rows);

  // Row geometry in cell units: SVG row index `y` and the row's first/last column.
  const rowSpan = (c: number, r: number) => {
    const { corner, rows } = clusters[c];
    const cols = width(rows);
    const count = rows[r];
    const top = corner === 'top-right';
    return {
      y: top ? r : rows.length - 1 - r,
      first: top ? cols - count : 0,
      last: top ? cols - 1 : count - 1,
    };
  };
  const cells = (c: number) =>
    clusters[c].rows.flatMap((_, r) => {
      const { y, first, last } = rowSpan(c, r);
      return Array.from({ length: last - first + 1 }, (_, i) => ({ x: first + i, y }));
    });

  // Pixel blocks wander: every hop, one block jumps to a free cell anywhere
  // in its cluster. `x`/`y` are cell indexes.
  type Kind = 'checker' | 'stripes' | 'bar';
  type Block = { cluster: number; x: number; y: number; kind: Kind };
  const clusterCells = clusters.map((_, c) => cells(c));
  const freeCell = (c: number, taken: Block[]) => {
    const free = clusterCells[c].filter(
      (cell) => !taken.some((b) => b.cluster === c && b.x === cell.x && b.y === cell.y),
    );
    return free[Math.floor(Math.random() * free.length)];
  };
  const kinds: Kind[] = ['checker', 'stripes', 'bar'];
  const initial: Block[] = [];
  for (const c of [0, 0, 0, 1, 1, 1]) {
    initial.push({ cluster: c, ...freeCell(c, initial), kind: kinds[initial.length % 3] });
  }
  let blocks = $state(initial);

  // Sub-pixel grid inside a block (8 × 8 at 10px).
  const sub = 10;
  const side = cell / sub;
  const checker = () => Array.from({ length: side * side }, () => Math.random() < 0.3);
  const stripes = () => Array.from({ length: side }, () => 1 + Math.floor(Math.random() * side));
  let tick = $state(0);
  let checkers = $state(Array.from({ length: 6 }, checker));
  let stripeLengths = $state(Array.from({ length: 6 }, stripes));

  onMount(() => {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const patterns = setInterval(() => {
      tick += 1;
      checkers = checkers.map(checker);
      stripeLengths = stripeLengths.map(stripes);
    }, 450);
    const hop = setInterval(() => {
      const index = Math.floor(Math.random() * blocks.length);
      const others = blocks.filter((_, i) => i !== index);
      const moved = { ...blocks[index], ...freeCell(blocks[index].cluster, others) };
      blocks = blocks.map((block, i) => (i === index ? moved : block));
    }, 700);
    return () => {
      clearInterval(patterns);
      clearInterval(hop);
    };
  });
</script>

<!-- First screen only, so the bottom-left cluster sits in view. -->
<div
  class="pointer-events-none absolute top-0 left-1/2 -z-10 h-[calc(100svh-64px)] w-screen -translate-x-1/2 overflow-hidden"
  aria-hidden="true"
>
  {#each clusters as cluster, c}
    {@const cols = width(cluster.rows)}
    <svg
      class={clsx('absolute', cluster.corner === 'top-right' ? 'top-0 right-0' : 'bottom-0 left-0')}
      width={cols * cell + 1}
      height={cluster.rows.length * cell + 1}
      viewBox="-0.5 -0.5 {cols * cell + 1} {cluster.rows.length * cell + 1}"
      shape-rendering="crispEdges"
    >
      {#each cells(c) as { x, y }}
        <rect
          class="fill-none stroke-[#2b312b]"
          x={x * cell}
          y={y * cell}
          width={cell}
          height={cell}
        />
      {/each}
      {#each blocks as block, b}
        {#if block.cluster === c}
          <g transform="translate({block.x * cell} {block.y * cell})">
            <rect class="fill-[#6fbf3a]" width={cell} height={cell} />
            {#if block.kind === 'checker'}
              {#each checkers[b] as lit, i}
                {#if lit}
                  <rect
                    class="fill-[#e4f3d4]"
                    x={(i % side) * sub}
                    y={Math.floor(i / side) * sub}
                    width={sub}
                    height={sub}
                  />
                {/if}
              {/each}
            {:else if block.kind === 'stripes'}
              {#each stripeLengths[b] as length, row}
                {#if row % 2 === 1}
                  <rect
                    class={row % 4 === 1 ? 'fill-[#e4f3d4]' : 'fill-[#c8ff5a]'}
                    y={row * sub + sub / 2 - 2}
                    width={length * sub}
                    height="4"
                  />
                {/if}
              {/each}
            {:else}
              <rect class="fill-[#e4f3d4]" y={(tick % side) * sub} width={cell} height={sub * 2} />
            {/if}
          </g>
        {/if}
      {/each}
    </svg>
  {/each}
</div>
