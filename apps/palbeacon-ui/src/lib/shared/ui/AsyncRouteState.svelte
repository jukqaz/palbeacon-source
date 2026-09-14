<script lang="ts">
  interface Props {
    state: 'loading' | 'error';
    title: string;
    message: string;
    accent?: 'normal' | 'brass';
    onRetry?: () => void | Promise<void>;
  }

  let { state, title, message, accent = 'normal', onRetry }: Props = $props();
</script>

<section
  class="route-state"
  class:error={state === 'error'}
  class:brass={accent === 'brass'}
  aria-live={state === 'error' ? 'assertive' : 'polite'}
  aria-busy={state === 'loading'}
  aria-label={title}
>
  {#if state === 'loading'}
    <span class="loader" aria-hidden="true"></span>
  {/if}
  <h1>{title}</h1>
  <p>{message}</p>
  {#if state === 'error' && onRetry}
    <button type="button" onclick={() => void onRetry()}>다시 시도</button>
  {/if}
</section>

<style>
  .route-state {
    display: grid;
    min-height: calc(100vh - 65px);
    min-height: calc(100dvh - 65px);
    place-items: center;
    align-content: center;
    padding: 32px;
    text-align: center;
  }

  .loader {
    width: 30px;
    height: 30px;
    border: 3px solid var(--border-strong);
    border-top-color: var(--tech-normal);
    animation: spin 800ms steps(8) infinite;
  }

  .brass .loader {
    border-top-color: var(--brass);
  }

  h1 {
    margin: 18px 0 6px;
    font-size: 1rem;
  }

  p {
    max-width: 42rem;
    margin: 0;
    color: var(--muted-strong);
    font-size: 0.8rem;
    line-height: 1.6;
  }

  button {
    min-height: 44px;
    margin-top: 18px;
    padding-inline: 15px;
    border: 1px solid var(--danger);
    border-radius: 5px;
    background: rgb(213 99 99 / 12%);
    cursor: pointer;
  }

  @keyframes spin {
    to {
      rotate: 1turn;
    }
  }
</style>
