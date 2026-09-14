<script lang="ts">
  import { page } from '$app/state';
  import NotFoundState from '$lib/app/shell/NotFoundState.svelte';
  import AsyncRouteState from '$lib/shared/ui/AsyncRouteState.svelte';
</script>

{#if page.status === 404}
  <NotFoundState path={page.url.pathname} showPath={false} />
{:else}
  <AsyncRouteState
    state="error"
    title="화면을 불러올 수 없습니다."
    message="다시 시도해 주세요."
    onRetry={() => window.location.reload()}
  />
{/if}
