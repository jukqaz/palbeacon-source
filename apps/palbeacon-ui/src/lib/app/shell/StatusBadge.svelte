<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    tone?: 'neutral' | 'success' | 'warning';
    children: Snippet;
  }

  let { tone = 'neutral', children }: Props = $props();
</script>

<span class="badge {tone}">{@render children()}</span>

<style>
  .badge {
    display: inline-flex;
    min-height: 24px;
    align-items: center;
    padding: 3px 8px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    color: var(--muted-strong);
    background: rgb(11 17 25 / 82%);
    font-size: 0.75rem;
    font-weight: 700;
    line-height: 1.2;
    letter-spacing: 0.02em;
  }

  .success {
    border-color: color-mix(in srgb, var(--success), transparent 45%);
    color: #a8e0b3;
  }

  .warning {
    border-color: color-mix(in srgb, var(--brass), transparent 35%);
    color: #e5ca7c;
  }
</style>
