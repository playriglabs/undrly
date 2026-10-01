<script lang="ts">
  import clsx from 'clsx';
  import Arrow from './Arrow.svelte';
  import { links } from '$lib/links';
  import { ui } from '$lib/ui';
  import { items, menuKeys, menus, type MenuKey } from '$lib/nav';

  let open = $state(false);
  let active = $state<MenuKey | null>(null);
  let closeTimer: ReturnType<typeof setTimeout> | undefined;

  const line = 'my-[5px] block h-px bg-ink transition-transform duration-[180ms]';

  function show(key: MenuKey) {
    clearTimeout(closeTimer);
    active = key;
  }
  // Short delay so the pointer can cross the gap between trigger and panel.
  function hide() {
    clearTimeout(closeTimer);
    closeTimer = setTimeout(() => (active = null), 120);
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key !== 'Escape') return;
    open = false;
    active = null;
  }}
/>
<a href="#main" class="fixed -top-25 left-3 z-100 bg-[#dbe4d3] p-3.75 text-[#1a2317] focus:top-3"
  >Skip to content</a
>
<!-- Hover only; keyboard users open menus via focus on the triggers. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<header class="sticky top-0 z-30 border-b border-[#ffffff12]" onmouseleave={hide}>
  <!-- Glass lives on its own layer: a backdrop-filter on <header> would become the
       backdrop root and stop the mega menu's blur from reaching the page. -->
  <div
    class="absolute inset-0 -z-10 bg-[#090b0a99] backdrop-blur-xl backdrop-saturate-150"
    aria-hidden="true"
  ></div>
  <div class="flex h-16 items-center justify-between gap-6 px-5 md:px-10">
    <div class="flex items-center gap-16">
      <a
        class="font-display text-4xl leading-none font-medium tracking-[-0.5px] max-md:text-[30px]"
        href="#main"
        aria-label="Undrly home">undrly</a
      >
      <nav class="hidden items-center gap-9 md:flex" aria-label="Main navigation">
        {#each items as item}
          {#if item.menu}
            {@const key = item.menu}
            <a
              class={clsx(
                'relative flex items-center text-[15px] font-[450] transition-colors duration-160 hover:text-ink',
                active === key ? 'text-ink' : 'text-[#a1a6a0]',
              )}
              href={item.href}
              aria-expanded={active === key}
              aria-controls="mega-menu"
              onmouseenter={() => show(key)}
              onfocus={() => show(key)}
            >
              <span
                class={clsx(
                  'absolute -left-3 size-1.25 bg-forest transition-opacity duration-200',
                  active === key ? 'opacity-100' : 'opacity-0',
                )}
                aria-hidden="true"
              ></span>
              {item.label}
            </a>
          {:else}
            <a
              class="text-[15px] font-[450] text-[#a1a6a0] transition-colors duration-160 hover:text-ink"
              href={item.href}
              onmouseenter={hide}
              onfocus={() => (active = null)}>{item.label}</a
            >
          {/if}
        {/each}
      </nav>
    </div>
    <div class="flex items-center gap-3">
      <a
        class={ui.buttonSmall}
        href={links.quickstart}
        onmouseenter={hide}
        onfocus={() => (active = null)}>Open App <Arrow /></a
      >
      <button
        class="h-9.5 w-9.5 px-1.75 py-2.5 md:hidden"
        aria-label={open ? 'Close navigation' : 'Open navigation'}
        aria-expanded={open}
        aria-controls="mobile-nav"
        onclick={() => (open = !open)}
      >
        <span class={clsx(line, open && 'translate-y-0.75 rotate-45')}></span>
        <span class={clsx(line, open && '-translate-y-0.75 -rotate-45')}></span>
      </button>
    </div>
  </div>

  <!-- Always mounted so height and opacity can transition both ways. -->
  <div
    id="mega-menu"
    class={clsx(
      'absolute inset-x-0 top-full hidden border-[#ffffff12] bg-[#090b0ab8] backdrop-blur-2xl backdrop-saturate-150 transition-[grid-template-rows,border-color,box-shadow] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] motion-reduce:transition-none md:grid',
      active
        ? 'grid-rows-[1fr] border-b shadow-[inset_0_1px_0_#ffffff0a,0_24px_48px_-24px_#000000b3]'
        : 'grid-rows-[0fr] border-transparent',
    )}
    inert={!active}
  >
    <div class="grid overflow-hidden">
      {#each menuKeys as key}
        {@const menu = menus[key]}
        <div
          class={clsx(
            'col-start-1 row-start-1 grid grid-cols-[1fr_minmax(300px,30%)] transition-[opacity,translate] duration-300 ease-out motion-reduce:transition-none',
            active === key
              ? 'translate-y-0 opacity-100'
              : 'pointer-events-none -translate-y-1.5 opacity-0',
          )}
          aria-hidden={active !== key}
        >
          <div class="px-10 pt-10 pb-12">
            <p class="font-display text-[28px] leading-none tracking-[-0.02em]">{menu.title}</p>
            <ul class="mt-9 grid grid-cols-2 gap-x-12 gap-y-8">
              {#each menu.entries as entry}
                <li>
                  <a class="group block" href={entry.href} onclick={() => (active = null)}>
                    <span
                      class="block text-[17px] font-[450] text-ink transition-colors duration-160 group-hover:text-forest"
                      >{entry.title}</span
                    >
                    <span class="mt-1 block text-[15px] leading-normal text-muted"
                      >{entry.copy}</span
                    >
                  </a>
                </li>
              {/each}
            </ul>
          </div>
          <div class="border-l border-dashed border-line-strong px-9 pt-10 pb-12">
            <p class="font-display text-[28px] leading-none tracking-[-0.02em]">
              {menu.feature.heading}
            </p>
            <a
              class="group mt-9 block border border-line bg-card p-6 transition-colors duration-160 hover:border-line-strong"
              href={menu.feature.href}
              onclick={() => (active = null)}
            >
              <span class="block text-[17px] font-[450] text-ink">{menu.feature.title}</span>
              <span class="mt-1 block text-[15px] leading-normal text-muted"
                >{menu.feature.copy}</span
              >
              <span class="{ui.textLink} mt-5 text-forest">{menu.feature.cta} <Arrow /></span>
            </a>
          </div>
        </div>
      {/each}
    </div>
  </div>

  {#if open}
    <nav
      id="mobile-nav"
      class="border-t border-line px-6 pt-2.5 pb-6 md:hidden"
      aria-label="Mobile navigation"
    >
      {#each items as item}<a
          class="group flex justify-between py-3.75"
          href={item.href}
          onclick={() => (open = false)}>{item.label} <Arrow /></a
        >{/each}
    </nav>
  {/if}
</header>
