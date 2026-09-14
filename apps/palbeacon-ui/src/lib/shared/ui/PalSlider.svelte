<script lang="ts">
  import { Slider } from 'bits-ui';

  interface Props {
    value: number;
    min: number;
    max: number;
    step: number;
    label: string;
    disabled?: boolean;
  }

  let { value = $bindable(), min, max, step, label, disabled = false }: Props = $props();
</script>

<Slider.Root
  class="pal-slider"
  type="single"
  bind:value
  {min}
  {max}
  {step}
  {disabled}
  aria-label={label}
>
  <Slider.Range class="pal-slider-range" />
  <Slider.Thumb class="pal-slider-thumb" index={0} aria-label={label} />
</Slider.Root>

<style>
  :global(.pal-slider) {
    position: relative;
    display: flex;
    width: 100%;
    height: 28px;
    align-items: center;
    touch-action: none;
  }

  :global(.pal-slider::before) {
    position: absolute;
    right: 0;
    left: 0;
    height: 5px;
    border: 1px solid var(--border-strong);
    border-radius: 2px;
    background: var(--void);
    content: '';
  }

  :global(.pal-slider-range) {
    position: absolute;
    height: 5px;
    border-radius: 2px;
    background: var(--accent);
  }

  :global(.pal-slider-thumb) {
    z-index: 1;
    width: 18px;
    height: 24px;
    border: 2px solid var(--text);
    border-radius: 3px;
    background: var(--accent-strong);
    box-shadow: 0 4px 14px rgb(0 0 0 / 38%);
  }

  :global(.pal-slider-thumb:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 3px;
  }

  :global(.pal-slider[data-disabled]) {
    opacity: 0.45;
  }
</style>
