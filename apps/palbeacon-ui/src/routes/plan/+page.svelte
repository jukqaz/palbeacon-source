<script lang="ts">
  import { browser } from '$app/environment';
  import { page } from '$app/state';
  import ToolsPage from '$lib/tools/ToolsPage.svelte';

  const initialOwnedOnly = $derived(browser && page.url.searchParams.get('mine') === '1');
</script>

<ToolsPage initialMode="team" {initialOwnedOnly} />
