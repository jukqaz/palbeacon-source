<script lang="ts">
  import { page } from '$app/state';
  import NotFoundState from '$lib/app/shell/NotFoundState.svelte';
</script>

<NotFoundState path={page.url.pathname} />
