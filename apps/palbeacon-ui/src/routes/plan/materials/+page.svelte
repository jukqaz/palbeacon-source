<script lang="ts">
  import { browser } from '$app/environment';
  import { page } from '$app/state';
  import ToolsPage from '$lib/tools/ToolsPage.svelte';

  const initialBuildingId = $derived(browser ? (page.url.searchParams.get('id') ?? '') : '');
</script>

<ToolsPage initialMode="materials" {initialBuildingId} />
