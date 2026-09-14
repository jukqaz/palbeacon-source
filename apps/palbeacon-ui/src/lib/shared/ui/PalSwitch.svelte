<script lang="ts">
  import { Switch } from 'bits-ui';

  interface Props {
    checked: boolean;
    label: string;
    disabled?: boolean;
  }

  let { checked = $bindable(), label, disabled = false }: Props = $props();
</script>

<Switch.Root class="pal-switch" bind:checked {disabled} aria-label={label}>
  <Switch.Thumb class="pal-switch-thumb" />
</Switch.Root>

<style>
  :global(.pal-switch) {
    position: relative;
    width: 48px;
    height: 28px;
    flex: 0 0 auto;
    padding: 3px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: var(--surface-strong);
    cursor: pointer;
  }

  :global(.pal-switch[data-state='checked']) {
    border-color: color-mix(in srgb, var(--accent), white 12%);
    background: var(--accent-strong);
  }

  :global(.pal-switch-thumb) {
    display: block;
    width: 20px;
    height: 20px;
    border-radius: 2px;
    background: var(--text);
    box-shadow: 0 2px 8px rgb(0 0 0 / 35%);
    transition: translate 140ms ease;
  }

  :global(.pal-switch[data-state='checked'] .pal-switch-thumb) {
    translate: 20px 0;
  }

  :global(.pal-switch:disabled) {
    cursor: not-allowed;
    opacity: 0.45;
  }
</style>
