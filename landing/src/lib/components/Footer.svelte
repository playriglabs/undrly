<script lang="ts">
  import { links } from '$lib/links';
  import { menuKeys, menus } from '$lib/nav';

  // Keep the footer aligned with the header navigation while giving it a
  // quieter, directory-like treatment.
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
      label: 'LinkedIn',
      href: links.linkedin,
      path: 'M20.45 20.45h-3.56v-5.57c0-1.33-.02-3.04-1.85-3.04-1.85 0-2.14 1.45-2.14 2.94v5.67H9.35V9h3.41v1.56h.05c.48-.9 1.64-1.85 3.37-1.85 3.6 0 4.27 2.37 4.27 5.46v6.28ZM5.34 7.43a2.06 2.06 0 1 1 0-4.13 2.06 2.06 0 0 1 0 4.13ZM7.12 20.45H3.56V9h3.56v11.45ZM22.22 0H1.77C.79 0 0 .77 0 1.73v20.54C0 23.23.79 24 1.77 24h20.45c.98 0 1.78-.77 1.78-1.73V1.73C24 .77 23.2 0 22.22 0Z',
    },
    {
      label: 'GitHub',
      href: links.github,
      path: 'M12 .3a12 12 0 0 0-3.79 23.39c.6.11.82-.26.82-.58v-2.1c-3.34.73-4.04-1.42-4.04-1.42-.55-1.39-1.33-1.76-1.33-1.76-1.09-.74.08-.73.08-.73 1.2.09 1.84 1.24 1.84 1.24 1.07 1.83 2.81 1.3 3.5.99.11-.78.42-1.31.76-1.61-2.67-.31-5.47-1.34-5.47-5.93 0-1.31.47-2.38 1.24-3.22-.13-.3-.54-1.52.11-3.18 0 0 1.01-.32 3.3 1.23a11.5 11.5 0 0 1 6 0c2.29-1.55 3.3-1.23 3.3-1.23.65 1.66.24 2.88.12 3.18.77.84 1.23 1.91 1.23 3.22 0 4.61-2.81 5.62-5.48 5.92.43.37.81 1.1.81 2.22v3.29c0 .32.22.7.83.58A12 12 0 0 0 12 .3Z',
    },
    {
      label: 'X',
      href: links.x,
      path: 'M17.75 3h3.07l-6.71 7.67L22 21h-6.18l-4.84-6.33L5.44 21H2.37l7.18-8.2L2 3h6.34l4.37 5.78L17.75 3Zm-1.08 16.18h1.7L7.4 4.73H5.58l11.09 14.45Z',
    },
  ];

  const year = new Date().getFullYear();
</script>

<footer class="border-t border-line bg-[#10160f]">
  <div class="px-5 md:px-10">
    <div class="grid grid-cols-2 pt-10 max-md:grid-cols-1 max-md:gap-y-12">
      <div>
        <a
          class="inline-block font-display text-4xl leading-none font-medium tracking-[-0.5px] max-md:text-[30px]"
          href="#main"
          aria-label="Undrly home">undrly</a
        >
        <p class="mt-5 max-w-100 text-[15px] leading-[1.65] text-muted">
          The underlying layer for every market. One normalized API for equities, tokenized stocks,
          crypto, FX, commodities and perpetuals.
        </p>
      </div>

      <nav class="grid grid-cols-2 gap-x-10 gap-y-11 sm:grid-cols-4" aria-label="Footer navigation">
        {#each columns as column}
          <div>
            <p class="text-[11px] tracking-[0.13em] text-[#6f776e] uppercase">
              {column.title}
            </p>
            <ul class="mt-5 space-y-3.5">
              {#each column.links as link}
                <li>
                  <a
                    class="text-[15px] text-[#c5c9c4] transition-colors duration-160 hover:text-white"
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
      class="mt-20 flex items-end justify-between gap-10 pb-10 max-md:mt-16 max-md:flex-col max-md:items-start"
    >
      <div class="flex gap-2.5" aria-label="Social links">
        {#each socials as social}
          <a
            class="flex size-11 items-center justify-center bg-card text-[#c5c9c4] transition-[background,color] duration-160 hover:bg-[#252c24] hover:text-white"
            href={social.href}
            target="_blank"
            rel="noopener noreferrer"
            aria-label={social.label}
          >
            <svg class="size-4.5" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d={social.path} />
            </svg>
          </a>
        {/each}
      </div>

      <div
        class="flex flex-wrap items-center justify-end gap-x-7 gap-y-2 text-[13px] text-[#6f776e] max-md:justify-start"
      >
        <p>© {year} Undrly</p>
        <a class="transition-colors duration-160 hover:text-ink" href={links.contact}>Support</a>
        <a class="transition-colors duration-160 hover:text-ink" href={links.docs}>Docs</a>
      </div>
    </div>
  </div>

  <!-- The wordmark, as wide as the page; decorative (the logo link above names it). -->
  <div class="overflow-hidden px-5 md:px-10" aria-hidden="true">
    <p
      class="mb-[-0.08em] text-center font-display text-[34.5vw] leading-[0.8] font-medium tracking-[-0.04em] text-forest select-none"
    >
      undrly
    </p>
  </div>
</footer>
