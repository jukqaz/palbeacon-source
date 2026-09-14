<script lang="ts">
  interface Props {
    size?: number;
    decorative?: boolean;
  }

  let { size = 32, decorative = false }: Props = $props();
</script>

<img
  class="mark"
  src="/generated/brand/app-mark-square-v3.png"
  alt={decorative ? '' : 'PalBeacon'}
  aria-hidden={decorative}
  width={size}
  height={size}
/>

<style>
  .mark {
    display: block;
    object-fit: contain;
    image-rendering: pixelated;
  }
</style>
