export interface UserNumberOptions {
  maximumFractionDigits?: number;
  minimumFractionDigits?: number;
  fallback?: string;
}

const formatters = new Map<string, Intl.NumberFormat>();

const formatterFor = (minimumFractionDigits: number, maximumFractionDigits: number) => {
  const key = `${minimumFractionDigits}:${maximumFractionDigits}`;
  const cached = formatters.get(key);
  if (cached) return cached;

  const formatter = new Intl.NumberFormat('ko-KR', {
    minimumFractionDigits,
    maximumFractionDigits,
  });
  formatters.set(key, formatter);
  return formatter;
};

/**
 * Formats a number for people rather than preserving storage precision.
 * Meaningful fractional digits remain, while trailing zeroes such as `.0` disappear.
 */
export const formatUserNumber = (
  value: number | null | undefined,
  { minimumFractionDigits = 0, maximumFractionDigits = 2, fallback = '—' }: UserNumberOptions = {},
): string => {
  if (value == null || !Number.isFinite(value)) return fallback;
  const normalized = Object.is(value, -0) ? 0 : value;
  return formatterFor(minimumFractionDigits, maximumFractionDigits).format(normalized);
};

export const formatModelScore = (value: number | null | undefined): string =>
  formatUserNumber(value, { maximumFractionDigits: 1 });

export const formatScale = (value: number | null | undefined): string =>
  `${formatUserNumber(value, { maximumFractionDigits: 2 })}×`;
