import { fireEvent, render, screen } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ownedPalOwnerUid, ownedPalSnapshotFixture, stagedSaveFixture } from './connection.fixture';
import OwnedPalPanel from './OwnedPalPanel.svelte';
import type { NativeOwnedPalSnapshot } from './types';
import {
  clearPersonalPalData,
  ownedSpeciesCount,
  personalPalData,
} from '$lib/personal/personal-data';

describe('OwnedPalPanel', () => {
  beforeEach(clearPersonalPalData);

  it('requires explicit inspection and explicit local-player selection', async () => {
    const inspect = vi
      .fn<(importId: string, ownerUid: string | null) => Promise<NativeOwnedPalSnapshot>>()
      .mockImplementation((_importId: string, ownerUid: string | null) =>
        Promise.resolve(ownedPalSnapshotFixture(ownerUid)),
      );
    const rememberSelection = vi.fn<() => Promise<void>>(() => Promise.resolve());
    render(OwnedPalPanel, {
      staged: stagedSaveFixture,
      parserReady: true,
      inspect,
      rememberSelection,
    });

    expect(inspect).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: '내 팰 열기' }));
    expect(inspect).toHaveBeenCalledWith('b0bc52eda611399d11efe26f', null);
    expect(await screen.findByRole('button', { name: /Arthur/ })).toBeInTheDocument();
    expect(screen.queryByText('라이바오')).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: /Arthur/ }));
    expect(inspect).toHaveBeenLastCalledWith('b0bc52eda611399d11efe26f', ownedPalOwnerUid);
    expect(await screen.findByText('라이바오')).toBeInTheDocument();
    expect(screen.queryByText('GrassPanda_Electric')).not.toBeInTheDocument();
    expect(screen.getByText('Lv.55')).toBeInTheDocument();
    expect(ownedSpeciesCount(get(personalPalData), 'GrassPanda_Electric')).toBe(1);
    expect(rememberSelection).toHaveBeenCalledWith('b0bc52eda611399d11efe26f', ownedPalOwnerUid);
    expect(screen.getByRole('link', { name: '내 팰 도감' })).toHaveAttribute(
      'href',
      '/pals/?mine=1',
    );
    expect(screen.getByRole('link', { name: '내 팰로 계획' })).toHaveAttribute(
      'href',
      '/plan/?mine=1',
    );
    expect(screen.queryByText(/월드 팰/u)).not.toBeInTheDocument();
  });

  it('keeps the action unavailable when the fixed parser bundle is missing', () => {
    const inspect =
      vi.fn<(importId: string, ownerUid: string | null) => Promise<NativeOwnedPalSnapshot>>();
    render(OwnedPalPanel, { staged: stagedSaveFixture, parserReady: false, inspect });
    expect(screen.queryByRole('region', { name: '내 팰' })).not.toBeInTheDocument();
    expect(screen.queryByText(/해석기|parser/i)).not.toBeInTheDocument();
    expect(inspect).not.toHaveBeenCalled();
  });

  it('lets the user continue beyond the first 24 owned Pals', async () => {
    const base = ownedPalSnapshotFixture(ownedPalOwnerUid);
    const sourcePal = base.pals[0]!;
    const inspect = vi.fn<
      (importId: string, ownerUid: string | null) => Promise<NativeOwnedPalSnapshot>
    >(() =>
      Promise.resolve({
        ...base,
        pals: Array.from({ length: 25 }, (_, index) => ({
          ...sourcePal,
          instance_id: `owned-pal-${index.toString()}`,
          nickname: `라이바오 ${String(index + 1)}`,
        })),
      }),
    );
    render(OwnedPalPanel, { staged: stagedSaveFixture, parserReady: true, inspect });

    await fireEvent.click(screen.getByRole('button', { name: '내 팰 열기' }));
    await fireEvent.click(await screen.findByRole('button', { name: /Arthur/ }));
    expect(await screen.findByRole('button', { name: '다음 1마리 보기' })).toBeInTheDocument();
    expect(screen.queryByText('라이바오 25')).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: '다음 1마리 보기' }));
    expect(screen.getByText('라이바오 25')).toBeInTheDocument();
  });
});
