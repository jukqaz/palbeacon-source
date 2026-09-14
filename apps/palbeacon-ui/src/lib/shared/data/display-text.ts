const underscoredInternalToken = /\b[A-Za-z][A-Za-z0-9]*_[A-Za-z0-9_]+\b/u;
const pascalCaseInternalToken = /\b(?:[A-Z][a-z0-9]+){2,}[A-Za-z0-9]*\b/u;
const missingKoreanRelationSubject = /^(?:의\s|이\(가\)\s|은\(는\)\s|을\(를\)\s)/u;

export const hasKoreanDisplayText = (value: string | null | undefined): value is string =>
  typeof value === 'string' && /[가-힣]/u.test(value);

export const safeGameDescription = (value: string | null | undefined): string | null => {
  if (!value) return null;
  if (missingKoreanRelationSubject.test(value.trim())) return null;
  const safeSentences = value
    .replaceAll('\n', ' ')
    .split(/(?<=[.!?])\s+/u)
    .filter(
      (sentence) =>
        !underscoredInternalToken.test(sentence) && !pascalCaseInternalToken.test(sentence),
    )
    .map((sentence) =>
      sentence
        .replace(/\s*\(\s*\)/gu, '')
        .replace(/\s{2,}/gu, ' ')
        .trim(),
    )
    .filter((sentence) => sentence.length > 0);
  return safeSentences.length > 0 ? safeSentences.join(' ') : null;
};
