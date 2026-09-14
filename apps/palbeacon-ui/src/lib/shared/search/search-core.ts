export const normalizeSearchText = (value: string): string =>
  value.normalize('NFKC').toLocaleLowerCase('ko-KR').replaceAll(/\s+/gu, ' ').trim();

export const normalizeCompactSearchText = (value: string): string =>
  normalizeSearchText(value).replaceAll(/[\s_\-·]+/gu, '');

export const compareEntityId = (left: string, right: string): number =>
  left.localeCompare(right, 'en-US');
