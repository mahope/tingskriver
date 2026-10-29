export interface LanguageOption {
  code: string;
  label: string;
}

/**
 * Language options offered in the summary language pickers.
 * Codes must stay in sync with `language_name_from_code` in
 * `frontend/src-tauri/src/summary/processor.rs`.
 */
export const LANGUAGE_OPTIONS: LanguageOption[] = [
  { code: 'en', label: 'Engelsk' },
  { code: 'zh', label: 'Kinesisk' },
  { code: 'zh-tw', label: 'Traditionelt kinesisk' },
  { code: 'de', label: 'Tysk' },
  { code: 'es', label: 'Spansk' },
  { code: 'ru', label: 'Russisk' },
  { code: 'ko', label: 'Koreansk' },
  { code: 'fr', label: 'Fransk' },
  { code: 'ja', label: 'Japansk' },
  { code: 'pt', label: 'Portugisisk' },
  { code: 'it', label: 'Italiensk' },
  { code: 'nl', label: 'Hollandsk' },
  { code: 'pl', label: 'Polsk' },
  { code: 'ar', label: 'Arabisk' },
  { code: 'hi', label: 'Hindi' },
  { code: 'ta', label: 'Tamil' },
  { code: 'tr', label: 'Tyrkisk' },
  { code: 'vi', label: 'Vietnamesisk' },
  { code: 'th', label: 'Thailandsk' },
  { code: 'id', label: 'Indonesisk' },
  { code: 'sv', label: 'Svensk' },
  { code: 'cs', label: 'Tjekkisk' },
  { code: 'da', label: 'Dansk' },
  { code: 'fi', label: 'Finsk' },
  { code: 'el', label: 'Græsk' },
  { code: 'he', label: 'Hebraisk' },
  { code: 'hu', label: 'Ungarsk' },
  { code: 'no', label: 'Norsk' },
  { code: 'ro', label: 'Rumænsk' },
  { code: 'uk', label: 'Ukrainsk' },
];

export const AUTO_VALUE = '__auto__' as const;

const SUPPORTED_CODES: ReadonlySet<string> = new Set(LANGUAGE_OPTIONS.map((o) => o.code));

/**
 * Normalises a raw locale string (from transcription or storage) into a code we
 * can translate into. Handles BCP-47 regional tags: `pt-BR` -> `pt`, `en_GB` -> `en`.
 * Returns null for unsupported languages so callers can fall back to English
 * rather than sending a code Rust will silently drop.
 */
export function normaliseLanguageCode(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const lower = raw.toLowerCase().replace(/_/g, '-');
  if (SUPPORTED_CODES.has(lower)) return lower;
  const base = lower.split('-')[0];
  if (SUPPORTED_CODES.has(base)) return base;
  return null;
}

export function labelForCode(code: string): string {
  return LANGUAGE_OPTIONS.find((l) => l.code === code)?.label ?? code;
}
