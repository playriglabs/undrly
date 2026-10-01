<script lang="ts">
  import { ui } from '$lib/ui';
  import Ornament from './Ornament.svelte';
  const features = [
    {
      kind: 'capture' as const,
      title: 'Built on the original.',
      body: 'Every source response is preserved before it becomes a normalized observation.',
      label: '01 — COLLECT',
    },
    {
      kind: 'modules' as const,
      title: 'Connected by design.',
      body: 'Instruments, units, and venues align. Eligible observations become a canonical quote.',
      label: '02 — NORMALIZE & AGGREGATE',
    },
    {
      kind: 'records' as const,
      title: 'Ready when you are.',
      body: 'One API reads consistent results from storage, without waiting on upstream providers.',
      label: '03 — SERVE',
    },
  ];
</script>

<section
  id="how-it-works"
  class="{ui.section} border-y border-line bg-[#0b0d0c]"
  aria-labelledby="pipeline-title"
>
  <div class={ui.shell}>
    <div class="{ui.sectionHeading} reveal">
      <div class={ui.headingRow}>
        <h2 id="pipeline-title" class={ui.h2}>From fragmented<br /><span>to connected.</span></h2>
        <p class={ui.sectionCopy}>
          Different sources, formats, and conventions. One clear path to usable market data.
        </p>
      </div>
    </div>
    <div class="reveal grid grid-cols-3 max-md:grid-cols-1">
      {#each features as feature}<article
          class="min-w-0 border-r border-line px-7.75 first:pl-0 last:border-0 last:pr-0 max-[1100px]:px-5.75 max-md:border-r-0 max-md:border-b max-md:px-0 max-md:pt-5 max-md:pb-8.5 max-md:last:border-b-0 max-md:last:pb-0"
        >
          <Ornament kind={feature.kind} />
          <div class="max-md:mx-auto max-md:max-w-97.5">
            <p class="mb-3.5 text-[8px] tracking-[1px] text-[#909a8c]">{feature.label}</p>
            <h3 class="mb-3.5 text-[26px] leading-tight tracking-[-0.02em]">{feature.title}</h3>
            <p class="max-w-[320px] text-sm leading-[1.85] text-muted max-md:max-w-none">
              {feature.body}
            </p>
          </div>
        </article>{/each}
    </div>
  </div>
</section>
