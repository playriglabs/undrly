<script lang="ts">
  let { diagonal = false } = $props<{ diagonal?: boolean }>();
</script>

<svg
  width="18"
  height="18"
  viewBox="0 0 24 24"
  fill="none"
  aria-hidden="true"
  class="transition-[translate] duration-180 group-hover:translate-x-0.5 group-hover:-translate-y-px"
  style:transform={diagonal ? 'rotate(-45deg)' : undefined}
>
  <path
    d="M5 12h14m-6-6 6 6-6 6"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
  />
</svg>
