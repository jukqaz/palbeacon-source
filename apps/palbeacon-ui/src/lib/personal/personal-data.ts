import { writable } from 'svelte/store';
import type { NativeOwnedPalSnapshot } from '$lib/connection/types';

interface OwnedSpeciesSummary {
  name_ko: string;
  count: number;
  highest_level: number;
}

export interface PersonalPalData {
  ready: boolean;
  owner_name: string | null;
  owned_by_species: Record<string, OwnedSpeciesSummary>;
}

const emptyPersonalPalData = (): PersonalPalData => ({
  ready: false,
  owner_name: null,
  owned_by_species: {},
});

export const personalPalData = writable<PersonalPalData>(emptyPersonalPalData());

export const clearPersonalPalData = (): void => personalPalData.set(emptyPersonalPalData());

export const applyOwnedPalSnapshot = (snapshot: NativeOwnedPalSnapshot): void => {
  const ownerUid = snapshot.selected_owner_uid;
  if (!ownerUid) {
    clearPersonalPalData();
    return;
  }

  const ownedBySpecies: Record<string, OwnedSpeciesSummary> = {};
  for (const pal of snapshot.pals) {
    const name = pal.species_name_ko?.trim();
    if (!name || pal.owner_uid !== ownerUid) continue;
    const current = ownedBySpecies[pal.species_id];
    ownedBySpecies[pal.species_id] = current
      ? {
          ...current,
          count: current.count + 1,
          highest_level: Math.max(current.highest_level, pal.level),
        }
      : { name_ko: name, count: 1, highest_level: pal.level };
  }

  personalPalData.set({
    ready: true,
    owner_name: snapshot.players.find((player) => player.uid === ownerUid)?.nickname.trim() || null,
    owned_by_species: ownedBySpecies,
  });
};

export const ownedSpeciesCount = (state: PersonalPalData, speciesId: string): number =>
  state.owned_by_species[speciesId]?.count ?? 0;
