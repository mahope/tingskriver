import { invoke } from '@tauri-apps/api/core';

export interface AppStyleSetting {
  process: string;
  style: 'normal' | 'raw';
}

export interface DictationSettings {
  enabled: boolean;
  shortcut: string;
  engine: 'auto' | 'local';
  syvEndpoint: string;
  syvModel: string;
  fallbackAfterMs: number;
  polishEnabled: boolean;
  polishProvider: 'builtin-ai' | 'ollama';
  polishModel: string;
  polishEndpoint: string;
  polishBudgetMs: number;
  historyEnabled: boolean;
  keepAudio: boolean;
  restoreClipboardMs: number;
  appStyles: AppStyleSetting[];
}

export interface DictationStatus {
  platformSupported: boolean;
  shortcutActive: boolean;
  hasSyvKey: boolean;
  localModelAvailable: boolean;
  lastError: string | null;
}

export interface DictionaryWord {
  id: number;
  word: string;
  aliases: string;
}

export interface SnippetRow {
  id: number;
  phrase: string;
  text: string;
}

export interface HistoryEntry {
  id: number;
  createdAt: number;
  text: string;
  app: string | null;
  style: string;
  engine: 'syv' | 'local';
  fallbackReason: string | null;
  latencyMs: number;
  audioMs: number;
  insertMethod: string | null;
  hasAudio: boolean;
}

export const HISTORY_CHANGED_EVENT = 'dictation-history-changed';

export const dictationApi = {
  getSettings: () => invoke<DictationSettings>('dictation_get_settings'),
  saveSettings: (settings: DictationSettings) => invoke<DictationSettings>('dictation_save_settings', { settings }),
  getStatus: () => invoke<DictationStatus>('dictation_get_status'),
  setSyvKey: (key: string) => invoke<void>('dictation_set_syv_key', { key }),
  deleteSyvKey: () => invoke<void>('dictation_delete_syv_key'),
  testSyv: () => invoke<{ latencyMs: number }>('dictation_test_syv'),
  previewCleanup: (text: string, style: string) => invoke<string>('dictation_preview_cleanup', { text, style }),
  listDictionary: () => invoke<DictionaryWord[]>('dictation_list_dictionary'),
  saveDictionaryWord: (id: number | null, word: string, aliases: string) =>
    invoke<number>('dictation_save_dictionary_word', { id, word, aliases }),
  deleteDictionaryWord: (id: number) => invoke<void>('dictation_delete_dictionary_word', { id }),
  listSnippets: () => invoke<SnippetRow[]>('dictation_list_snippets'),
  saveSnippet: (id: number | null, phrase: string, text: string) =>
    invoke<number>('dictation_save_snippet', { id, phrase, text }),
  deleteSnippet: (id: number) => invoke<void>('dictation_delete_snippet', { id }),
  listHistory: (limit = 500) => invoke<HistoryEntry[]>('dictation_list_history', { limit }),
  deleteHistory: (id: number) => invoke<void>('dictation_delete_history', { id }),
  clearHistory: () => invoke<void>('dictation_clear_history'),
};

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** "1,2 s" or "850 ms" with Danish decimal comma. */
export function formatMs(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toLocaleString('da-DK', { maximumFractionDigits: 1 })} s`;
}
