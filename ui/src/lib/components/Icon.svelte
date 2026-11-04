<script lang="ts">
  import { ICON_PATHS, type IconName } from './icons';

  // Decorative Tabler outline icon: the meaning is always carried by adjacent text (P1-00E).
  let { name, size = 20 }: { name: IconName; size?: number } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#each ICON_PATHS[name] as d (d)}<path {d} />{/each}
</svg>
