<script lang="ts">
  import { browser } from '$app/environment';
  import { page } from '$app/state';
  import CatalogPage from '$lib/catalog/CatalogPage.svelte';

  const initialQuery = $derived(browser ? (page.url.searchParams.get('q') ?? '') : '');
  const initialSelectedId = $derived(browser ? (page.url.searchParams.get('id') ?? '') : '');
</script>

<CatalogPage scope="passive_skill" title="패시브 스킬" {initialQuery} {initialSelectedId} />
