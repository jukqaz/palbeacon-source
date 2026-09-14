<script lang="ts">
  import { Checkbox } from 'bits-ui';

  interface Props {
    checked: boolean;
    label: string;
    disabled?: boolean;
  }

  let { checked = $bindable(), label, disabled = false }: Props = $props();
</script>

<Checkbox.Root class="pal-checkbox" bind:checked {disabled} aria-label={label}>
  {#snippet children({ checked: isChecked })}
    <span aria-hidden="true">{isChecked ? '✓' : ''}</span>
  {/snippet}
</Checkbox.Root>

<style>
  :global(.pal-checkbox) {
    display: inline-grid;
    width: 25px;
    height: 25px;
    flex: 0 0 auto;
    place-items: center;
    padding: 0;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    color: var(--void);
    background: var(--surface-strong);
    cursor: pointer;
    font-size: 0.9rem;
    font-weight: 950;
  }

  :global(.pal-checkbox[data-state='checked']) {
    border-color: var(--brass);
    background: var(--brass);
  }

  :global(.pal-checkbox:disabled) {
    cursor: not-allowed;
    opacity: 0.45;
  }
</style>
