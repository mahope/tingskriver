'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useRouter } from 'next/navigation';
import { Copy, Trash2, Settings, Keyboard } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import {
  dictationApi,
  errorText,
  formatMs,
  HISTORY_CHANGED_EVENT,
  type DictationSettings,
  type DictationStatus,
  type HistoryEntry,
} from './api';

const timeFormat = new Intl.DateTimeFormat('da-DK', {
  day: 'numeric',
  month: 'short',
  hour: '2-digit',
  minute: '2-digit',
});

function engineLabel(entry: HistoryEntry): string {
  return entry.engine === 'syv' ? 'Syv.ai' : 'Lokal model';
}

function StatusLine({ status, settings }: { status: DictationStatus | null; settings: DictationSettings | null }) {
  if (!status || !settings) return null;
  let text: string;
  let tone = 'text-muted-foreground';
  if (!status.platformSupported) {
    text = 'Genvejen og indsættelse virker kun på Windows. Historik, ordbog og snippets kan stadig redigeres.';
  } else if (!settings.enabled) {
    text = 'Diktering er slået fra under Indstillinger.';
  } else if (status.lastError) {
    text = status.lastError;
    tone = 'text-destructive';
  } else if (!status.hasSyvKey && !status.localModelAvailable) {
    text = 'Der er hverken en Syv.ai-nøgle eller en lokal model. Tilføj en af dem under Indstillinger.';
    tone = 'text-warning';
  } else {
    const engine = settings.engine === 'local' || !status.hasSyvKey ? 'den lokale model' : 'Syv.ai (lokal model som reserve)';
    text = `Genvejen er klar. Transskriberes med ${engine}.`;
  }
  return <p className={`text-sm ${tone}`} role="status">{text}</p>;
}

export function DictationHistory() {
  const router = useRouter();
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [settings, setSettings] = useState<DictationSettings | null>(null);
  const [status, setStatus] = useState<DictationStatus | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);

  const load = useCallback(async () => {
    try {
      const [history, loadedSettings, loadedStatus] = await Promise.all([
        dictationApi.listHistory(),
        dictationApi.getSettings(),
        dictationApi.getStatus(),
      ]);
      setEntries(history ?? []);
      setSettings(loadedSettings);
      setStatus(loadedStatus);
    } catch (error) {
      setEntries([]);
      toast.error('Historikken kunne ikke hentes', { description: errorText(error) });
    }
  }, []);

  useEffect(() => {
    load();
    const unlisten = listen(HISTORY_CHANGED_EVENT, () => load());
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [load]);

  const copy = async (entry: HistoryEntry) => {
    try {
      await navigator.clipboard.writeText(entry.text);
      toast.success('Teksten er kopieret');
    } catch (error) {
      toast.error('Teksten kunne ikke kopieres', { description: errorText(error) });
    }
  };

  const remove = async (entry: HistoryEntry) => {
    try {
      await dictationApi.deleteHistory(entry.id);
      setEntries((prev) => prev?.filter((e) => e.id !== entry.id) ?? null);
    } catch (error) {
      toast.error('Dikteringen kunne ikke slettes', { description: errorText(error) });
    }
  };

  const clearAll = async () => {
    if (!confirmClear) {
      setConfirmClear(true);
      return;
    }
    setConfirmClear(false);
    try {
      await dictationApi.clearHistory();
      setEntries([]);
      toast.success('Historikken er ryddet');
    } catch (error) {
      toast.error('Historikken kunne ikke ryddes', { description: errorText(error) });
    }
  };

  const shortcut = settings?.shortcut ?? 'Ctrl+Win';

  return (
    <div className="h-screen overflow-y-auto custom-scrollbar">
      <div className="max-w-3xl mx-auto px-8 py-8 space-y-6">
        <header className="space-y-3">
          <div className="flex items-start justify-between gap-4">
            <h1 className="text-3xl text-foreground">Diktering</h1>
            <Button variant="outline" onClick={() => router.push('/settings?tab=diktering')}>
              <Settings aria-hidden="true" />
              Indstillinger
            </Button>
          </div>
          <p className="text-foreground">
            Hold <kbd className="px-1.5 py-0.5 rounded border border-border bg-muted text-sm font-sans">{shortcut}</kbd>, tal, og slip.
            Teksten bliver skrevet ind dér, hvor markøren står. Tryk to gange hurtigt for at diktere uden at holde tasterne nede,
            og tryk igen, når du er færdig.
          </p>
          <StatusLine status={status} settings={settings} />
        </header>

        <section aria-labelledby="dictation-history-heading" className="space-y-3">
          <div className="flex items-center justify-between gap-4">
            <h2 id="dictation-history-heading" className="text-xl text-foreground">Seneste dikteringer</h2>
            {entries && entries.length > 0 && (
              <Button variant={confirmClear ? 'destructive' : 'ghost'} size="sm" onClick={clearAll} onBlur={() => setConfirmClear(false)}>
                {confirmClear ? 'Klik igen for at rydde alt' : 'Ryd historik'}
              </Button>
            )}
          </div>

          {settings && !settings.historyEnabled && (
            <p className="text-sm text-muted-foreground">Historikken er slået fra, så nye dikteringer gemmes ikke.</p>
          )}

          {entries === null ? (
            <div className="space-y-3" aria-hidden="true">
              {[0, 1, 2].map((i) => (
                <div key={i} className="h-20 rounded-lg bg-muted motion-safe:animate-pulse" />
              ))}
            </div>
          ) : entries.length === 0 ? (
            <div className="rounded-lg border border-dashed border-border px-6 py-10 text-center">
              <Keyboard className="mx-auto h-6 w-6 text-muted-foreground" aria-hidden="true" />
              <p className="mt-3 font-medium text-foreground">Ingen dikteringer endnu</p>
              <p className="mt-1 text-sm text-muted-foreground">
                Stil markøren i et tekstfelt, hold {shortcut} og sig noget. Så dukker det op her.
              </p>
            </div>
          ) : (
            <ul className="space-y-3">
              {entries.map((entry) => (
                <li key={entry.id} className="rounded-lg border border-border bg-card p-4">
                  <p className="whitespace-pre-wrap break-words text-card-foreground">{entry.text}</p>
                  <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
                    <p className="text-sm text-muted-foreground tabular-nums">
                      <time dateTime={new Date(entry.createdAt).toISOString()}>{timeFormat.format(new Date(entry.createdAt))}</time>
                      {entry.app ? <> · {entry.app}</> : null}
                      {' · '}
                      {engineLabel(entry)}
                      {' · '}
                      <span title="Tid fra du slap tasterne, til teksten var indsat">{formatMs(entry.latencyMs)}</span>
                      {entry.fallbackReason ? (
                        <span className="text-warning" title={entry.fallbackReason}> · reserve brugt</span>
                      ) : null}
                    </p>
                    <div className="flex items-center gap-1">
                      <Button variant="ghost" size="sm" onClick={() => copy(entry)} aria-label="Kopiér teksten">
                        <Copy aria-hidden="true" />
                        Kopiér
                      </Button>
                      <Button variant="ghost" size="sm" onClick={() => remove(entry)} aria-label="Slet dikteringen">
                        <Trash2 aria-hidden="true" />
                      </Button>
                    </div>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}
