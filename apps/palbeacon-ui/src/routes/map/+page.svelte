<script lang="ts">
  import { page } from '$app/state';
  import { browser } from '$app/environment';
  import MapPage from '$lib/map/MapPage.svelte';

  const initialQuery = $derived(browser ? (page.url.searchParams.get('q') ?? '') : '');
</script>

<MapPage {initialQuery} />
