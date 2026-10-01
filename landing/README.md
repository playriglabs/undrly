# Undrly landing

Four-section marketing page ending at the closing CTA, without a footer. Dark theme, locally served brand
fonts (Inter Tight for headings, Pilat Book for body text), an animated grid
backdrop, and a deliberately empty dashboard preview frame in the hero.

## Develop

```sh
cd landing
bun install --frozen-lockfile
bun run dev
```

Open the local URL printed by Vite (normally `http://127.0.0.1:5173`).

## Validate and build

```sh
bun run check
bun run lint
bun run build
bun run preview
```

SvelteKit prerenders to `build/` using adapter-static. Before publishing, set
`VITE_SITE_URL` to the actual HTTPS origin and rebuild so social image metadata
points to the correct host. No deployment has been configured yet.

## Implementation

- SvelteKit / Svelte 5 with strict TypeScript, Tailwind CSS 4.
- GSAP + ScrollTrigger: subtle entrances and the three isometric diagrams in
  “How Undrly works”. Loops pause offscreen; reduced motion disables animation.
- `clsx` for conditional classes; `ts-pattern` for branching content and states.
- A mobile menu and normal anchor navigation. The Developers & Agents, Market
  Coverage, and Financial Identity sections have been removed; the provenance
  section became a four-card capabilities grid.
- No database or upstream provider calls. Examples describe data semantics and
  are explicitly illustrative. API, Rust, and contract code remain separate.
- Styling is Tailwind utilities in the markup; shared class strings live in
  `src/lib/ui.ts`. `src/app.css` only holds fonts, theme tokens, and base rules.
- External documentation links target the repo's verified `main` branch.
- Pilat Book is copied from `../brands/` into `static/fonts/`; headings use Inter Tight.

## Social asset

`static/og.png` was generated with the built-in ImageGen tool and inspected.
Final edit prompt: “Edit this social card to dark mode. Preserve exact layout,
landscape proportions, spacing, text placement, line breaks, and stacked
isometric planes. Background #090b0a; near-white wordmark and headline; grey
supporting line; thin slate-grey illustration with restrained pale green
accents. Use Inter Tight Regular or a closely matching compact modern sans-serif
throughout. Preserve exact text: ‘undrly’, ‘Every market. One clear interface.’,
‘One normalized API across every market.’ No other words, glow, gradients,
borders, or added objects.” Raster lettering is an approximation; actual page
typography uses the font files.
