import { get } from 'svelte/store';
import { beforeEach, describe, expect, it } from 'vitest';
import { ownedPalOwnerUid, ownedPalSnapshotFixture } from '$lib/connection/connection.fixture';
import {
  applyOwnedPalSnapshot,
  clearPersonalPalData,
  ownedSpeciesCount,
  personalPalData,
} from './personal-data';

describe('personal Pal data projection', () => {
  beforeEach(clearPersonalPalData);

  it('exposes only the explicitly selected owner and verified Korean species', () => {
    applyOwnedPalSnapshot(ownedPalSnapshotFixture(ownedPalOwnerUid));

    const state = get(personalPalData);
    expect(state.ready).toBe(true);
    expect(state.owner_name).toBe('Arthur');
    expect(ownedSpeciesCount(state, 'GrassPanda_Electric')).toBe(1);
    expect(JSON.stringify(state)).not.toContain('bbbbbbbb-cccc-dddd-eeee-ffffffffffff');
  });

  it('clears the projection until a local player is selected', () => {
    applyOwnedPalSnapshot(ownedPalSnapshotFixture(ownedPalOwnerUid));
    applyOwnedPalSnapshot(ownedPalSnapshotFixture(null));
    expect(get(personalPalData)).toEqual({
      ready: false,
      owner_name: null,
      owned_by_species: {},
    });
  });
});
