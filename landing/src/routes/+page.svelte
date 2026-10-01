<script lang="ts">
  let { data } = $props();
  import Header from '$lib/components/Header.svelte';
  import Footer from '$lib/components/Footer.svelte';
  import Arrow from '$lib/components/Arrow.svelte';
  import HeroBackdrop from '$lib/components/HeroBackdrop.svelte';
  import Pipeline from '$lib/components/Pipeline.svelte';
  import Coverage from '$lib/components/Coverage.svelte';
  import Markets from '$lib/components/Markets.svelte';
  import CodeExamples from '$lib/components/CodeExamples.svelte';
  import Capabilities from '$lib/components/Capabilities.svelte';
  import Pricing from '$lib/components/Pricing.svelte';
  import { links } from '$lib/links';
  import { ui } from '$lib/ui';
  import { onMount } from 'svelte';

  // Explicit build-time origin; never infer a public URL from request headers.
  const siteOrigin = import.meta.env.VITE_SITE_URL || 'http://127.0.0.1:5173';
  const socialImage = new URL('/og.png', siteOrigin).href;

  let root: HTMLElement;
  onMount(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    // GSAP's package entry must not be evaluated by Node during SSR.
    void import('$lib/motion').then(({ animateLanding }) => {
      if (disposed) return;
      cleanup = animateLanding(root);
    });
    return () => {
      disposed = true;
      cleanup?.();
    };
  });
</script>

<svelte:head>
  <title>Undrly — Every market. One clear interface.</title>
  <meta
    name="description"
    content="One normalized API for equities, crypto, FX, commodities, and perpetuals. Clear identities, traceable data, and tools for applications and AI agents."
  />
  <meta property="og:title" content="Undrly — Every market. One clear interface." />
  <meta
    property="og:description"
    content="One normalized API across every market. Clear identities. Traceable data."
  />
  <meta property="og:type" content="website" />
  <meta property="og:image" content={socialImage} />
  <meta property="og:image:alt" content="Undrly — Every market. One clear interface." />
  <meta name="twitter:card" content="summary_large_image" />
  <meta name="twitter:title" content="Undrly — Every market. One clear interface." />
  <meta
    name="twitter:description"
    content="One normalized API across every market. Clear identities. Traceable data."
  />
  <meta name="twitter:image" content={socialImage} />
</svelte:head>

<Header />
<main id="main" class="overflow-x-clip" bind:this={root}>
  <section class="{ui.shell} relative isolate pt-25 max-md:pt-16.25" aria-labelledby="hero-title">
    <HeroBackdrop />
    <h1
      id="hero-title"
      data-hero-reveal
      class="text-[clamp(58px,6.4vw,88px)] leading-[1.07] tracking-[-0.045em] max-md:text-[clamp(40px,8.5vw,64px)] [&_span]:text-[#a1a89f]"
    >
      The underlying layer<br /><span>for every market.</span>
    </h1>
    <div data-hero-reveal class="mt-8 flex flex-col gap-7 max-md:mt-6.25">
      <p
        class="max-w-138.75 text-[17px] leading-[1.7] text-muted max-md:max-w-112.5 max-md:text-[15px]"
      >
        Bring equities, crypto, FX, commodities, and perpetuals into one normalized API. Clear
        identities. Traceable data.
      </p>
    </div>
    <div
      data-hero-reveal
      aria-hidden="true"
      class="mt-17.5 aspect-[16/8.1] w-full border border-[#2c312c] bg-[#101310] shadow-[0_0_0_7px_#1216116b,0_22px_85px_-25px_#7f9b6120] max-md:mt-10.5 max-md:aspect-4/3"
    ></div>
  </section>
  <Pipeline />
  <Capabilities />
  <Markets />
  <Coverage />
  <CodeExamples highlighted={data.highlighted} />
  <Pricing />
  <section
    class="{ui.shell} border-t border-line pt-24.5 pb-29 text-center max-md:pt-18 max-md:pb-20"
    aria-labelledby="closing-title"
  >
    <div class="relative mx-auto mb-10 h-18.5 w-21.25" aria-hidden="true">
      {#each ['top-6 border-[#33442c]', 'top-[13px] border-[#465d3b]', 'top-0.5 border-[#66795a]'] as plane}<span
          class="{plane} absolute left-3.25 h-10.5 w-14.5 rounded-sm border bg-paper transform-[rotate(-30deg)_skewX(30deg)_scaleY(0.86)]"
        ></span>{/each}
    </div>
    <h2
      id="closing-title"
      class="reveal text-[clamp(45px,5.1vw,70px)] leading-[1.1] tracking-[-0.045em] max-md:text-[45px] [&_span]:text-[#929b90]"
    >
      One integration.<br /><span>More possibilities.</span>
    </h2>
    <p class="reveal mx-auto mt-7 mb-8 text-base leading-[1.75] text-muted max-md:text-[15px]">
      Spend less time reconciling market data. Start building with a common foundation.
    </p>
    <div class="flex flex-wrap justify-center items-center gap-8 reveal">
      <a class={ui.button} href={links.quickstart}>Open App <Arrow diagonal /></a><a
        class={ui.textLink}
        href={links.docs}>Explore the docs <Arrow /></a
      >
    </div>
  </section>
</main>
<Footer />
