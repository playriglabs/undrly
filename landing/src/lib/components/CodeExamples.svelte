<script lang="ts">
  import clsx from 'clsx';
  import { fade } from 'svelte/transition';
  import Arrow from './Arrow.svelte';
  import { examples } from '$lib/code-examples';
  import { links } from '$lib/links';
  import { ui } from '$lib/ui';

  // Shiki HTML per example, rendered at build time (see routes/+page.server.ts).
  let { highlighted }: { highlighted: Record<string, string> } = $props();

  let active = $state(0);
  const current = $derived(examples[active]);

  function onTabKey(event: KeyboardEvent) {
    const move = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
    if (!move) return;
    event.preventDefault();
    active = (active + move + examples.length) % examples.length;
    document.getElementById(`example-tab-${examples[active].id}`)?.focus();
  }
</script>

<section id="developers" class={ui.section} aria-labelledby="developers-title">
  <div class="{ui.shell} grid items-start gap-16 max-lg:gap-12 lg:grid-cols-[1fr_minmax(0,640px)]">
    <div class="reveal">
      <h2 id="developers-title" class={ui.h2}>
        <span>Use Undrly with</span><br />
        {#key current.id}<span class="text-ink!" in:fade={{ duration: 220 }}>{current.name}</span
          >{/key}
      </h2>
      <p class="{ui.sectionCopy} mt-7">
        Read-only HTTP routes and an MCP server. No client library required.
      </p>
    </div>

    <div class="reveal border border-line bg-[#0d100e]">
      <div class="grid grid-cols-5 border-b border-line" role="tablist" aria-label="Code examples">
        {#each examples as example, index}
          <button
            id="example-tab-{example.id}"
            class={clsx(
              'flex h-16 items-center justify-center border-r border-line transition-colors duration-160 last:border-r-0 max-md:h-13',
              active === index ? 'bg-card text-ink' : 'text-[#7d857c] hover:text-ink',
            )}
            role="tab"
            aria-selected={active === index}
            aria-controls="example-panel"
            tabindex={active === index ? 0 : -1}
            onclick={() => (active = index)}
            onkeydown={onTabKey}
            aria-label={example.tab}
            title={example.tab}
          >
            <svg
              class="size-6"
              viewBox="0 0 24 24"
              fill={example.stroke ? 'none' : 'currentColor'}
              stroke={example.stroke ? 'currentColor' : 'none'}
              stroke-width="1.6"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"><path d={example.icon} /></svg
            >
          </button>
        {/each}
      </div>

      <div
        id="example-panel"
        class="relative min-h-105 p-8 pb-22 max-md:min-h-95 max-md:p-5 max-md:pb-18"
        role="tabpanel"
        aria-labelledby="example-tab-{current.id}"
      >
        {#key current.id}
          <div
            class="overflow-x-auto text-[13.5px] leading-[1.9] max-md:text-[12px] [&_pre]:bg-transparent!"
            in:fade={{ duration: 220 }}
          >
            {@html highlighted[current.id]}
          </div>
        {/key}
        <a
          class="group absolute right-6 bottom-6 inline-flex items-center gap-3 border border-line-strong bg-[#0d100e] px-3.5 py-2 text-sm text-[#a1a6a0] transition-colors duration-160 hover:text-ink max-md:right-4 max-md:bottom-4"
          href={links.docs}>Read the docs <Arrow diagonal /></a
        >
      </div>
    </div>
  </div>
</section>
