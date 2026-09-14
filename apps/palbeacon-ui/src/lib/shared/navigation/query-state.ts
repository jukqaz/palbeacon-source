export const replaceCurrentQueryParameter = (name: string, value: string | null): void => {
  if (typeof window === 'undefined') return;
  const url = new URL(window.location.href);
  if (value === null) url.searchParams.delete(name);
  else url.searchParams.set(name, value);
  window.history.replaceState(window.history.state, '', url);
};
