<script lang="ts">
  import { ICONS, type IconName } from '$lib/design/icons';

  interface Props {
    name: IconName;
    /** Rendered size in px. The 24px grid scales cleanly to 16, 20, and 24. */
    size?: number;
    strokeWidth?: number;
    class?: string;
  }

  let { name, size = 20, strokeWidth = 1.5, class: className = '' }: Props = $props();
</script>

<!-- Always decorative. The accessible name belongs to the control that owns the
     icon, never to the icon itself, so a screen reader announces "Delete page"
     rather than "trash graphic, Delete page". -->
<svg
  class={className}
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  <path d={ICONS[name]} />
</svg>
