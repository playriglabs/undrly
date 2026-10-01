<script lang="ts">
  import clsx from 'clsx';
  import { ui } from '$lib/ui';
  import Arrow from './Arrow.svelte';
  import { links } from '$lib/links';

  // Line icons for benefit rows (16px, stroke).
  const icons = {
    code: 'M5.5 4 2 8l3.5 4M10.5 4 14 8l-3.5 4M9 3 7 13',
    agent:
      'M4 4h8v8H4zM6.5 1.5V4M9.5 1.5V4M6.5 12v2.5M9.5 12v2.5M1.5 6.5H4M1.5 9.5H4M12 6.5h2.5M12 9.5h2.5',
    gauge: 'M2.5 11a5.5 5.5 0 1 1 11 0M8 11l2.5-3.5M1.5 13.5h13',
    globe:
      'M8 1.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM1.5 8h13M8 1.5c-2.2 2-2.2 11 0 13M8 1.5c2.2 2 2.2 11 0 13',
    candles: 'M4 2v2.5M4 10v4M2.5 4.5h3V10h-3zM11 1.5V5M11 11.5v3M9.5 5h3v6.5h-3z',
    trail: 'M3 3v6a3 3 0 0 0 3 3h7M10 9l3 3-3 3',
    bank: 'M1.5 6 8 2l6.5 4M3 6.5v6M6 6.5v6M10 6.5v6M13 6.5v6M1.5 14h13',
    sliders: 'M3 2v12M8 2v12M13 2v12M1.5 5h3M6.5 10h3M11.5 6h3',
    server: 'M2 2.5h12v4.5H2zM2 9h12v4.5H2zM4.5 4.75h.01M4.5 11.25h.01',
    chat: 'M2 3h12v8H7l-3.5 3v-3H2z',
    clock: 'M8 1.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM8 4.5V8l2.5 1.5',
  };
  type Icon = keyof typeof icons;
  type Group = { label: string; items: [Icon, string][] };

  // Request quotas are placeholders until accounts and metering exist.
  const plans: {
    name: string;
    tagline: string;
    price: string;
    period: string;
    groups: Group[];
  }[] = [
    {
      name: 'Starter',
      tagline: 'Try the core API free for 7 days.',
      price: 'Free',
      period: '7-day trial',
      groups: [
        {
          label: 'DATA ACCESS',
          items: [
            ['code', 'REST API access with API key'],
            ['clock', '7-day free trial'],
            ['gauge', '10,000 requests / month'],
            ['globe', 'Core markets across 5 asset classes'],
            ['agent', 'Read-only MCP server for AI agents'],
          ],
        },
        { label: 'SUPPORT', items: [['chat', 'Community support']] },
      ],
    },
    {
      name: 'Pro',
      tagline: 'For builders and early teams.',
      price: '$20',
      period: '/month',
      groups: [
        {
          label: 'DATA ACCESS',
          items: [
            ['code', 'REST API access with API key'],
            ['gauge', '250,000 requests / month'],
            ['globe', 'Full universes: crypto top 100, S&P 500, FX majors, every Perp'],
            ['candles', 'Candles, reference rates and perpetual funding'],
            ['agent', 'Read-only MCP server for AI agents'],
          ],
        },
        { label: 'SUPPORT', items: [['chat', 'Email support']] },
      ],
    },
    {
      name: 'Growth',
      tagline: 'For products in production.',
      price: '$99',
      period: '/month',
      groups: [
        {
          label: 'DATA ACCESS',
          items: [
            ['code', 'REST API access with API key'],
            ['gauge', '2,000,000 requests / month'],
            ['globe', 'Everything in Pro'],
            ['trail', 'Source provenance behind every quote'],
            ['bank', 'Central-bank reference rates, including Asia, Southeast Asia, MENA FX'],
          ],
        },
        { label: 'SUPPORT', items: [['chat', 'Priority support']] },
      ],
    },
    {
      name: 'Scale',
      tagline: 'For platforms and data teams.',
      price: 'Custom',
      period: '',
      groups: [
        {
          label: 'DATA ACCESS',
          items: [
            ['code', 'Custom API volume'],
            ['globe', 'Everything in Growth'],
            ['sliders', 'Custom universes and sources'],
            ['server', 'Self-hosted deployment'],
          ],
        },
        { label: 'SUPPORT', items: [['chat', 'Dedicated support channel']] },
      ],
    },
  ];
</script>

<section id="pricing" class={ui.section} aria-labelledby="pricing-title">
  <div class={ui.shell}>
    <div class="reveal mb-18 grid grid-cols-2 gap-16 max-md:mb-10 max-md:grid-cols-1 max-md:gap-2">
      <p class="text-2xl">Pricing</p>
      <div>
        <h2 id="pricing-title" class={ui.h2}>
          Pricing that scales<br /><span>with your markets.</span>
        </h2>
        <div class="mt-10 flex flex-wrap items-center gap-4">
          <a class={ui.button} href={links.quickstart}>Get started <Arrow /></a>
          <a class={ui.buttonGhost} href={links.contact}>Contact us</a>
        </div>
      </div>
    </div>
    <div class="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4 reveal">
      {#each plans as plan}<article class="flex flex-col border border-line-strong p-8">
          <h3 class="text-[26px] tracking-[-0.02em]">{plan.name}</h3>
          <p class="mt-2 text-[15px] text-muted">{plan.tagline}</p>
          <p class="mt-9 mb-8 font-display text-[40px] leading-none tracking-[-0.03em]">
            {plan.price}{#if plan.period}<span
                class="ml-2 font-sans text-sm tracking-normal text-muted">{plan.period}</span
              >{/if}
          </p>
          {#each plan.groups as group, g}<!-- Support sits at the card bottom so it lines up across plans. -->
            <div class={clsx('border-t border-line-strong pt-6', g === 0 ? 'pb-6' : 'mt-auto')}>
              <p class="mb-4 text-[11px] font-medium tracking-[0.08em] text-ink">{group.label}</p>
              <ul class="grid gap-3">
                {#each group.items as [icon, text], i}<li
                    class={clsx(
                      'flex gap-3 text-sm leading-normal',
                      i === 0 ? 'font-inter text-[15px] font-semibold text-ink' : 'text-[#c9cfc5]',
                    )}
                  >
                    <svg
                      class="mt-0.5 h-4 w-4 shrink-0 text-[#9aa596]"
                      viewBox="0 0 16 16"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.25"
                      stroke-linecap="round"
                      stroke-linejoin="round"><path d={icons[icon]} /></svg
                    >{text}
                  </li>{/each}
              </ul>
            </div>{/each}
        </article>{/each}
    </div>
  </div>
</section>
