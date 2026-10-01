import { codeToHtml } from 'shiki';
import { examples } from '$lib/code-examples';
import { codeTheme } from '$lib/code-theme';

// Prerendered: Shiki runs at build time and never ships to the browser.
export async function load() {
  const entries = await Promise.all(
    examples.map(
      async (example) =>
        [
          example.id,
          await codeToHtml(example.code, { lang: example.lang, theme: codeTheme }),
        ] as const,
    ),
  );
  return { highlighted: Object.fromEntries(entries) };
}
