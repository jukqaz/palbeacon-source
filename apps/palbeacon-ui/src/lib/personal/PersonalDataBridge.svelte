<script lang="ts">
  import { clearPersonalPalData, applyOwnedPalSnapshot } from './personal-data';
  import { inspectNativeStagedSave, loadNativeAppSettings } from '$lib/connection/native';

  interface Props {
    enabled: boolean;
  }

  let { enabled }: Props = $props();

  $effect(() => {
    if (!enabled) {
      clearPersonalPalData();
      return;
    }

    let active = true;
    void (async () => {
      try {
        const document = await loadNativeAppSettings();
        const importId = document.settings.selected_import_id;
        const ownerUid = document.settings.selected_owner_uid;
        if (!importId || !ownerUid) {
          if (active) clearPersonalPalData();
          return;
        }
        const snapshot = await inspectNativeStagedSave(importId, ownerUid);
        if (active) applyOwnedPalSnapshot(snapshot);
      } catch {
        if (active) clearPersonalPalData();
      }
    })();

    return () => {
      active = false;
    };
  });
</script>
