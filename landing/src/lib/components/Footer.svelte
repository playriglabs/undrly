<script lang="ts">
  import { links } from '$lib/links';
  import { menuKeys, menus } from '$lib/nav';
  import { ui } from '$lib/ui';

  // Same destinations as the header: one column per mega menu, plus the flat links.
  const columns = [
    ...menuKeys.map((key) => ({
      title: menus[key].title,
      links: menus[key].entries.map((entry) => ({ label: entry.title, href: entry.href })),
    })),
    {
      title: 'Get started',
      links: [
        { label: 'Pricing', href: '#pricing' },
        { label: 'Quickstart', href: links.quickstart },
      ],
    },
  ];
  const socials = [
    {
      label: 'Undrly on X',
      href: links.x,
      icon: 'M17.75 3h3.07l-6.71 7.67L22 21h-6.18l-4.84-6.33L5.44 21H2.37l7.18-8.2L2 3h6.34l4.37 5.78L17.75 3Zm-1.08 16.18h1.7L7.4 4.73H5.58l11.09 14.45Z',
    },
    {
      label: 'Undrly on LinkedIn',
      href: links.linkedin,
      icon: 'M20.45 20.45h-3.56v-5.57c0-1.33-.02-3.04-1.85-3.04-1.85 0-2.14 1.45-2.14 2.94v5.67H9.35V9h3.41v1.56h.05c.48-.9 1.64-1.85 3.37-1.85 3.6 0 4.27 2.37 4.27 5.46v6.28ZM5.34 7.43a2.06 2.06 0 1 1 0-4.13 2.06 2.06 0 0 1 0 4.13ZM7.12 20.45H3.56V9h3.56v11.45ZM22.22 0H1.77C.79 0 0 .77 0 1.73v20.54C0 23.23.79 24 1.77 24h20.45c.98 0 1.78-.77 1.78-1.73V1.73C24 .77 23.2 0 22.22 0Z',
    },
  ];
  const year = new Date().getFullYear();
</script>

<footer class="relative overflow-hidden border-t border-line bg-[#070908]">
  <div class={ui.shell}>
    <div
      class="grid gap-16 pt-20 pb-16 max-md:gap-12 max-md:pt-14 lg:grid-cols-[minmax(260px,1fr)_2fr]"
    >
      <div class="flex flex-col items-start">
        <a
          class="font-display text-4xl leading-none font-medium tracking-[-0.5px]"
          href="#main"
          aria-label="Undrly home">undrly</a
        >
        <p class="mt-5 max-w-75 text-[15px] leading-[1.7] text-muted">
          The underlying layer for every market. One normalized API.
        </p>
        <div class="mt-6 flex items-center gap-2.5">
          {#each socials as social}
            <a
              class="flex size-9 items-center justify-center border border-line text-[#a1a6a0] transition-colors duration-160 hover:border-line-strong hover:text-ink"
              href={social.href}
              target="_blank"
              rel="noopener noreferrer"
              aria-label={social.label}
            >
              <svg class="size-4" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"
                ><path d={social.icon} /></svg
              >
            </a>
          {/each}
        </div>
      </div>

      <nav class="grid grid-cols-2 gap-x-8 gap-y-12 sm:grid-cols-4" aria-label="Footer navigation">
        {#each columns as column}
          <div>
            <p class="text-[11px] tracking-[0.08em] text-[#6f776e] uppercase">
              {column.title}
            </p>
            <ul class="mt-5 space-y-3.5">
              {#each column.links as link}
                <li>
                  <a
                    class="text-[14px] text-[#a1a6a0] transition-colors duration-160 hover:text-ink"
                    href={link.href}>{link.label}</a
                  >
                </li>
              {/each}
            </ul>
          </div>
        {/each}
      </nav>
    </div>

    <div
      class="flex flex-wrap items-center justify-between gap-x-8 gap-y-4 border-t border-dashed border-line-strong py-7 text-[13px] text-[#6f776e]"
    >
      <p>© {year} Undrly. Market data for information only, not investment advice.</p>
      <div class="flex items-center gap-6">
        <a class="transition-colors duration-160 hover:text-ink" href={links.contact}>Support</a>
        <a class="transition-colors duration-160 hover:text-ink" href={links.docs}>Docs</a>
      </div>
    </div>
  </div>

  <!-- Oversized wordmark, cropped by the footer edge. -->
  <p
    class="pointer-events-none mb-[-0.24em] text-center font-display text-[clamp(150px,26vw,500px)] leading-[0.8] font-medium tracking-[-0.04em] text-[#161e14] select-none"
    aria-hidden="true"
  >
    undrly
  </p>
</footer>
