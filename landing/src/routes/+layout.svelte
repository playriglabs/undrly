<script lang="ts">
  import '@fontsource-variable/inter-tight';
  import '../app.css';
  let { children } = $props();
</script>

<svelte:head>
  <link
    rel="preload"
    href="/fonts/Pilat-Book.woff2"
    as="font"
    type="font/woff2"
    crossorigin="anonymous"
  />
  <link
    rel="preload"
    href="/fonts/Reckless-Regular.woff2"
    as="font"
    type="font/woff2"
    crossorigin="anonymous"
  />
  <link
    rel="preload"
    href="/fonts/Reckless-Medium.woff2"
    as="font"
    type="font/woff2"
    crossorigin="anonymous"
  />
</svelte:head>

{@render children()}
