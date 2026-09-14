import { writable } from 'svelte/store';

export type WorkspaceEntryKind = 'pal' | 'item' | 'technology' | 'building' | 'poi' | 'plan';

export interface WorkspaceEntry {
  kind: WorkspaceEntryKind;
  id: string;
  name_ko: string;
  href: string;
  image_path?: string | null;
}

export interface WorkspaceState {
  recent: WorkspaceEntry[];
  saved: WorkspaceEntry[];
}

const STORAGE_KEY = 'palbeacon.workspace.v1';
const RECENT_LIMIT = 12;
const SAVED_LIMIT = 24;
const kinds = new Set<WorkspaceEntryKind>(['pal', 'item', 'technology', 'building', 'poi', 'plan']);
const emptyState = (): WorkspaceState => ({ recent: [], saved: [] });
const keyFor = (entry: Pick<WorkspaceEntry, 'kind' | 'id'>): string => `${entry.kind}:${entry.id}`;

const isEntry = (value: unknown): value is WorkspaceEntry => {
  if (typeof value !== 'object' || value === null) return false;
  const entry = value as Partial<WorkspaceEntry>;
  return (
    typeof entry.kind === 'string' &&
    kinds.has(entry.kind as WorkspaceEntryKind) &&
    typeof entry.id === 'string' &&
    entry.id.trim().length > 0 &&
    typeof entry.name_ko === 'string' &&
    entry.name_ko.trim().length > 0 &&
    typeof entry.href === 'string' &&
    entry.href.startsWith('/') &&
    (entry.image_path === undefined ||
      entry.image_path === null ||
      (typeof entry.image_path === 'string' && entry.image_path.startsWith('/')))
  );
};

const normalizeEntries = (value: unknown, limit: number): WorkspaceEntry[] =>
  Array.isArray(value) ? value.filter(isEntry).slice(0, limit) : [];

const readState = (): WorkspaceState => {
  if (typeof window === 'undefined') return emptyState();
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return emptyState();
    const value = JSON.parse(raw) as Partial<WorkspaceState>;
    return {
      recent: normalizeEntries(value.recent, RECENT_LIMIT),
      saved: normalizeEntries(value.saved, SAVED_LIMIT),
    };
  } catch {
    return emptyState();
  }
};

export const workspace = writable<WorkspaceState>(readState());

if (typeof window !== 'undefined') {
  workspace.subscribe((state) => {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    } catch {
      // Private or full storage must not block the current game-data task.
    }
  });
}

export const rememberWorkspaceEntry = (entry: WorkspaceEntry): void => {
  if (!isEntry(entry)) return;
  workspace.update((state) => ({
    ...state,
    recent: [
      entry,
      ...state.recent.filter((candidate) => keyFor(candidate) !== keyFor(entry)),
    ].slice(0, RECENT_LIMIT),
  }));
};

export const toggleWorkspaceEntry = (entry: WorkspaceEntry): void => {
  if (!isEntry(entry)) return;
  workspace.update((state) => {
    const saved = state.saved.some((candidate) => keyFor(candidate) === keyFor(entry));
    return {
      recent: [
        entry,
        ...state.recent.filter((candidate) => keyFor(candidate) !== keyFor(entry)),
      ].slice(0, RECENT_LIMIT),
      saved: saved
        ? state.saved.filter((candidate) => keyFor(candidate) !== keyFor(entry))
        : [entry, ...state.saved.filter((candidate) => keyFor(candidate) !== keyFor(entry))].slice(
            0,
            SAVED_LIMIT,
          ),
    };
  });
};

export const removeWorkspaceEntry = (entry: Pick<WorkspaceEntry, 'kind' | 'id'>): void => {
  workspace.update((state) => ({
    ...state,
    saved: state.saved.filter((candidate) => keyFor(candidate) !== keyFor(entry)),
  }));
};

export const clearWorkspace = (): void => workspace.set(emptyState());

export const recentWorkspaceEntries = (state: WorkspaceState): WorkspaceEntry[] => {
  const savedKeys = new Set(state.saved.map(keyFor));
  return state.recent.filter((entry) => !savedKeys.has(keyFor(entry)));
};

export const workspaceKindLabel = (kind: WorkspaceEntryKind): string =>
  ({
    pal: '팰',
    item: '아이템',
    technology: '기술',
    building: '건축물',
    poi: '지도 위치',
    plan: '계획',
  })[kind];
