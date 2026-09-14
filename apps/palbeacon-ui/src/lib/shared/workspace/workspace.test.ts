import { get } from 'svelte/store';
import { beforeEach, describe, expect, it } from 'vitest';
import {
  clearWorkspace,
  recentWorkspaceEntries,
  rememberWorkspaceEntry,
  removeWorkspaceEntry,
  toggleWorkspaceEntry,
  workspace,
} from './workspace';

const pal = {
  kind: 'pal' as const,
  id: 'SheepBall',
  name_ko: '도로롱',
  href: '/pals/?q=%EB%8F%84%EB%A1%9C%EB%A1%B1&id=SheepBall',
  image_path: '/generated/game/pals/T_SheepBall_icon_normal.webp',
};

describe('PalBeacon local workspace', () => {
  beforeEach(() => {
    localStorage.clear();
    clearWorkspace();
  });

  it('keeps a recently viewed exact entity once', () => {
    rememberWorkspaceEntry(pal);
    rememberWorkspaceEntry(pal);
    expect(get(workspace).recent).toEqual([pal]);
  });

  it('toggles a saved entry without exposing a second copy', () => {
    toggleWorkspaceEntry(pal);
    toggleWorkspaceEntry(pal);
    expect(get(workspace).saved).toEqual([]);
    expect(get(workspace).recent).toEqual([pal]);
  });

  it('removes only the saved entry and preserves recent context', () => {
    toggleWorkspaceEntry(pal);
    removeWorkspaceEntry(pal);
    expect(get(workspace).saved).toEqual([]);
    expect(get(workspace).recent).toEqual([pal]);
  });

  it('keeps saved entries out of the separate recent-only surface', () => {
    toggleWorkspaceEntry(pal);
    expect(recentWorkspaceEntries(get(workspace))).toEqual([]);

    removeWorkspaceEntry(pal);
    expect(recentWorkspaceEntries(get(workspace))).toEqual([pal]);
  });
});
