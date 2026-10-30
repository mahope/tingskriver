'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { Plus, Trash2, Pencil } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import {
  dictationApi,
  errorText,
  formatMs,
  type AppStyleSetting,
  type DictationSettings as Settings,
  type DictationStatus,
  type DictionaryWord,
  type SnippetRow,
} from './api';

const selectClass =
  'h-11 min-h-[44px] rounded-md border border-input bg-background px-3 text-sm text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring';

function Section({ title, description, children }: { title: string; description?: React.ReactNode; children: React.ReactNode }) {
  const id = `dict-${title.toLowerCase().replace(/[^a-zæøå]+/g, '-')}`;
  return (
    <section aria-labelledby={id} className="space-y-4">
      <div>
        <h3 id={id} className="text-xl text-foreground">{title}</h3>
        {description ? <p className="text-sm text-muted-foreground mt-1">{description}</p> : null}
      </div>
      {children}
    </section>
  );
}

function ToggleRow({
  id,
  label,
  hint,
  checked,
  onChange,
  disabled,
}: {
  id: string;
  label: string;
  hint: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-6">
      <div className="flex-1">
        <label htmlFor={id} className="font-medium text-foreground">{label}</label>
        <div id={`${id}-hint`} className="text-sm text-muted-foreground">{hint}</div>
      </div>
      <Switch id={id} aria-describedby={`${id}-hint`} checked={checked} onCheckedChange={onChange} disabled={disabled} />
    </div>
  );
}

export function DictationSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [status, setStatus] = useState<DictationStatus | null>(null);
  const [words, setWords] = useState<DictionaryWord[]>([]);
  const [snippets, setSnippets] = useState<SnippetRow[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [shortcut, setShortcut] = useState('');
  const [syvKey, setSyvKey] = useState('');
  const [testing, setTesting] = useState(false);
  const [styles, setStyles] = useState<AppStyleSetting[]>([]);
  const [previewInput, setPreviewInput] = useState('øh hej Mads komma kan vi mødes i morgen spørgsmålstegn');
  const [preview, setPreview] = useState('');

  const [wordId, setWordId] = useState<number | null>(null);
  const [word, setWord] = useState('');
  const [aliases, setAliases] = useState('');
  const [snippetId, setSnippetId] = useState<number | null>(null);
  const [phrase, setPhrase] = useState('');
  const [snippetText, setSnippetText] = useState('');

  const loadAll = useCallback(async () => {
    try {
      const [s, st, w, sn] = await Promise.all([
        dictationApi.getSettings(),
        dictationApi.getStatus(),
        dictationApi.listDictionary(),
        dictationApi.listSnippets(),
      ]);
      setSettings(s);
      setShortcut(s.shortcut);
      setStyles(s.appStyles);
      setStatus(st);
      setWords(w ?? []);
      setSnippets(sn ?? []);
      setLoadError(null);
    } catch (error) {
      setLoadError(errorText(error));
    }
  }, []);

  useEffect(() => {
    loadAll();
  }, [loadAll]);

  const save = async (patch: Partial<Settings>, success = 'Indstillingen er gemt') => {
    if (!settings) return;
    try {
      const saved = await dictationApi.saveSettings({ ...settings, ...patch });
      setSettings(saved);
      setShortcut(saved.shortcut);
      setStyles(saved.appStyles);
      setStatus(await dictationApi.getStatus());
      toast.success(success);
    } catch (error) {
      toast.error('Indstillingen kunne ikke gemmes', { description: errorText(error) });
      setShortcut(settings.shortcut);
    }
  };

  const saveKey = async () => {
    try {
      await dictationApi.setSyvKey(syvKey);
      setSyvKey('');
      setStatus(await dictationApi.getStatus());
      toast.success('Nøglen er gemt i Windows Legitimationsstyring');
    } catch (error) {
      toast.error('Nøglen kunne ikke gemmes', { description: errorText(error) });
    }
  };

  const removeKey = async () => {
    try {
      await dictationApi.deleteSyvKey();
      setStatus(await dictationApi.getStatus());
      toast.success('Nøglen er fjernet');
    } catch (error) {
      toast.error('Nøglen kunne ikke fjernes', { description: errorText(error) });
    }
  };

  const testKey = async () => {
    setTesting(true);
    try {
      const result = await dictationApi.testSyv();
      toast.success('Syv.ai svarer', { description: `Svartid ${formatMs(result.latencyMs)}` });
    } catch (error) {
      toast.error('Syv.ai-testen fejlede', { description: errorText(error) });
    } finally {
      setTesting(false);
    }
  };

  const runPreview = async () => {
    try {
      setPreview(await dictationApi.previewCleanup(previewInput, 'normal'));
    } catch (error) {
      toast.error('Renskrivningen kunne ikke vises', { description: errorText(error) });
    }
  };

  const saveWord = async (event: React.FormEvent) => {
    event.preventDefault();
    try {
      await dictationApi.saveDictionaryWord(wordId, word, aliases);
      setWordId(null);
      setWord('');
      setAliases('');
      setWords(await dictationApi.listDictionary());
    } catch (error) {
      toast.error('Ordet kunne ikke gemmes', { description: errorText(error) });
    }
  };

  const removeWord = async (id: number) => {
    try {
      await dictationApi.deleteDictionaryWord(id);
      setWords(await dictationApi.listDictionary());
    } catch (error) {
      toast.error('Ordet kunne ikke slettes', { description: errorText(error) });
    }
  };

  const saveSnippetForm = async (event: React.FormEvent) => {
    event.preventDefault();
    try {
      await dictationApi.saveSnippet(snippetId, phrase, snippetText);
      setSnippetId(null);
      setPhrase('');
      setSnippetText('');
      setSnippets(await dictationApi.listSnippets());
    } catch (error) {
      toast.error('Snippet kunne ikke gemmes', { description: errorText(error) });
    }
  };

  const removeSnippet = async (id: number) => {
    try {
      await dictationApi.deleteSnippet(id);
      setSnippets(await dictationApi.listSnippets());
    } catch (error) {
      toast.error('Snippet kunne ikke slettes', { description: errorText(error) });
    }
  };

  if (loadError) {
    return (
      <div className="max-w-2xl p-4 border border-destructive/30 rounded-lg bg-destructive/10 text-sm text-destructive" role="alert">
        Indstillingerne for diktering kunne ikke hentes: {loadError}
      </div>
    );
  }

  if (!settings || !status) {
    return (
      <div className="animate-pulse max-w-2xl" aria-hidden="true">
        <div className="h-4 bg-muted rounded w-1/4 mb-4"></div>
        <div className="h-8 bg-muted rounded mb-4"></div>
      </div>
    );
  }

  const stylesChanged = JSON.stringify(styles) !== JSON.stringify(settings.appStyles);

  return (
    <div className="max-w-2xl space-y-10">
      <div>
        <h2 className="text-2xl text-foreground mb-1">Diktering</h2>
        <p className="text-sm text-muted-foreground">
          Hold genvejen, tal, og slip. Teksten bliver renskrevet og skrevet ind i det program, du arbejder i.
        </p>
        {!status.platformSupported && (
          <p className="mt-3 text-sm text-warning">Genvejen og indsættelse virker kun på Windows.</p>
        )}
        {status.lastError && <p className="mt-3 text-sm text-destructive" role="alert">{status.lastError}</p>}
      </div>

      <Section title="Genvej">
        <ToggleRow
          id="dict-enabled"
          label="Diktering med genvej"
          hint="Lyt efter genvejen i alle programmer, også når Tingskriver er skjult"
          checked={settings.enabled}
          onChange={(enabled) => save({ enabled })}
        />
        <div className="space-y-2">
          <label htmlFor="dict-shortcut" className="font-medium text-foreground">Tastekombination</label>
          <div className="flex gap-2">
            <Input
              id="dict-shortcut"
              value={shortcut}
              onChange={(e) => setShortcut(e.target.value)}
              aria-describedby="dict-shortcut-hint"
              className="max-w-xs"
            />
            <Button variant="outline" onClick={() => save({ shortcut })} disabled={shortcut === settings.shortcut}>
              Gem genvej
            </Button>
          </div>
          <p id="dict-shortcut-hint" className="text-sm text-muted-foreground">
            Fx Ctrl+Win, Ctrl+Alt+Space eller F9. Hold for at tale, slip for at indsætte. To hurtige tryk dikterer
            uden at holde tasterne, til du trykker igen.
          </p>
        </div>
      </Section>

      <Section
        title="Transskription"
        description="Med en Syv.ai-nøgle bruges Syv.ai. Svarer Syv.ai ikke hurtigt nok, eller fejler det, bruges den lokale model automatisk."
      >
        <div className="space-y-2">
          <label htmlFor="dict-engine" className="font-medium text-foreground">Tale til tekst</label>
          <select
            id="dict-engine"
            className={`${selectClass} w-full`}
            value={settings.engine}
            onChange={(e) => save({ engine: e.target.value as Settings['engine'] })}
          >
            <option value="auto">Syv.ai, med lokal model som reserve</option>
            <option value="local">Kun lokal model (lyden forlader ikke computeren)</option>
          </select>
          {!status.localModelAvailable && (
            <p className="text-sm text-warning">
              Der er ingen lokal Parakeet-model endnu. Hent den under Transskription, så der er en reserve.
            </p>
          )}
        </div>

        {settings.engine === 'auto' && (
          <div className="space-y-2">
            <label htmlFor="dict-syv-key" className="font-medium text-foreground">Syv.ai-nøgle</label>
            <p className="text-sm text-muted-foreground">
              {status.hasSyvKey
                ? 'Der er gemt en nøgle i Windows Legitimationsstyring.'
                : 'Ingen nøgle. Indtil der er en, bruges den lokale model.'}
            </p>
            <div className="flex flex-wrap gap-2">
              <Input
                id="dict-syv-key"
                type="password"
                autoComplete="off"
                placeholder={status.hasSyvKey ? 'Indsæt en ny nøgle for at skifte' : 'hv_…'}
                value={syvKey}
                onChange={(e) => setSyvKey(e.target.value)}
                className="max-w-xs"
              />
              <Button variant="outline" onClick={saveKey} disabled={!syvKey.trim()}>Gem nøgle</Button>
              {status.hasSyvKey && (
                <>
                  <Button variant="outline" onClick={testKey} disabled={testing}>{testing ? 'Tester…' : 'Test'}</Button>
                  <Button variant="ghost" onClick={removeKey}>Fjern</Button>
                </>
              )}
            </div>
            <div className="flex items-center gap-2 pt-2">
              <label htmlFor="dict-fallback" className="text-sm text-foreground">Brug lokal model, hvis Syv.ai er længere end</label>
              <select
                id="dict-fallback"
                className={selectClass}
                value={settings.fallbackAfterMs}
                onChange={(e) => save({ fallbackAfterMs: Number(e.target.value) })}
              >
                {[1000, 1500, 2000, 3000, 5000].map((ms) => (
                  <option key={ms} value={ms}>{formatMs(ms)}</option>
                ))}
              </select>
            </div>
          </div>
        )}
      </Section>

      <Section
        title="Renskrivning"
        description="Fyldord som øh og altså fjernes, og du kan sige tegn: punktum, komma, spørgsmålstegn, udråbstegn, kolon, ny linje og nyt afsnit."
      >
        <div className="space-y-2">
          <label htmlFor="dict-preview" className="font-medium text-foreground">Prøv det</label>
          <div className="flex gap-2">
            <Input id="dict-preview" value={previewInput} onChange={(e) => setPreviewInput(e.target.value)} />
            <Button variant="outline" onClick={runPreview}>Vis</Button>
          </div>
          {preview && (
            <p className="rounded-md border border-border bg-muted px-3 py-2 text-sm text-foreground whitespace-pre-wrap" aria-live="polite">
              {preview}
            </p>
          )}
        </div>
        <ToggleRow
          id="dict-polish"
          label="Polering med sprogmodel"
          hint="Retter grammatik og selvrettelser. Tager den mere end 2 sekunder, indsættes teksten uden polering. En lokal model er ofte for langsom."
          checked={settings.polishEnabled}
          onChange={(polishEnabled) => save({ polishEnabled })}
        />
        {settings.polishEnabled && (
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <label htmlFor="dict-polish-provider" className="text-sm font-medium text-foreground">Udbyder</label>
              <select
                id="dict-polish-provider"
                className={`${selectClass} w-full`}
                value={settings.polishProvider}
                onChange={(e) => save({ polishProvider: e.target.value as Settings['polishProvider'] })}
              >
                <option value="builtin-ai">Indbygget lokal model</option>
                <option value="ollama">Ollama</option>
              </select>
            </div>
            <div className="space-y-1">
              <label htmlFor="dict-polish-model" className="text-sm font-medium text-foreground">Model</label>
              <Input
                id="dict-polish-model"
                defaultValue={settings.polishModel}
                placeholder={settings.polishProvider === 'ollama' ? 'qwen2.5:3b' : 'Samme som opsummering'}
                onBlur={(e) => e.target.value !== settings.polishModel && save({ polishModel: e.target.value })}
              />
            </div>
            {settings.polishProvider === 'ollama' && (
              <div className="space-y-1 sm:col-span-2">
                <label htmlFor="dict-polish-endpoint" className="text-sm font-medium text-foreground">Ollama-adresse</label>
                <Input
                  id="dict-polish-endpoint"
                  defaultValue={settings.polishEndpoint}
                  placeholder="http://localhost:11434"
                  onBlur={(e) => e.target.value !== settings.polishEndpoint && save({ polishEndpoint: e.target.value })}
                />
              </div>
            )}
          </div>
        )}
      </Section>

      <Section
        title="Stil pr. program"
        description="I kodeprogrammer og terminaler skrives teksten råt: intet stort begyndelsesbogstav, og kun øh fjernes. Navnet er programfilen, fx Code.exe."
      >
        <ul className="space-y-2">
          {styles.map((row, index) => (
            <li key={index} className="flex gap-2">
              <Input
                aria-label="Program"
                value={row.process}
                onChange={(e) => setStyles(styles.map((s, i) => (i === index ? { ...s, process: e.target.value } : s)))}
              />
              <select
                aria-label={`Stil for ${row.process || 'programmet'}`}
                className={selectClass}
                value={row.style}
                onChange={(e) =>
                  setStyles(styles.map((s, i) => (i === index ? { ...s, style: e.target.value as AppStyleSetting['style'] } : s)))
                }
              >
                <option value="raw">Rå</option>
                <option value="normal">Normal</option>
              </select>
              <Button
                variant="ghost"
                size="icon"
                className="h-11 w-11"
                aria-label={`Fjern ${row.process || 'rækken'}`}
                onClick={() => setStyles(styles.filter((_, i) => i !== index))}
              >
                <Trash2 aria-hidden="true" />
              </Button>
            </li>
          ))}
        </ul>
        <div className="flex gap-2">
          <Button variant="outline" onClick={() => setStyles([...styles, { process: '', style: 'raw' }])}>
            <Plus aria-hidden="true" />
            Tilføj program
          </Button>
          <Button onClick={() => save({ appStyles: styles })} disabled={!stylesChanged}>Gem programmer</Button>
        </div>
      </Section>

      <Section
        title="Ordbog"
        description="Navne og fagord, som skal staves rigtigt. Ordene sendes med som stikord til Syv.ai, og stavemåder, du skriver under 'Høres som', rettes til ordet."
      >
        <form onSubmit={saveWord} className="grid gap-2 sm:grid-cols-[1fr_1fr_auto] items-end">
          <div className="space-y-1">
            <label htmlFor="dict-word" className="text-sm font-medium text-foreground">Ord</label>
            <Input id="dict-word" value={word} onChange={(e) => setWord(e.target.value)} placeholder="Mahoje" />
          </div>
          <div className="space-y-1">
            <label htmlFor="dict-aliases" className="text-sm font-medium text-foreground">Høres som (valgfrit)</label>
            <Input id="dict-aliases" value={aliases} onChange={(e) => setAliases(e.target.value)} placeholder="maho je, mahoye" />
          </div>
          <Button type="submit" disabled={!word.trim()}>{wordId === null ? 'Tilføj' : 'Gem'}</Button>
        </form>
        {words.length === 0 ? (
          <p className="text-sm text-muted-foreground">Ordbogen er tom.</p>
        ) : (
          <ul className="divide-y divide-border rounded-lg border border-border">
            {words.map((w) => (
              <li key={w.id} className="flex items-center justify-between gap-3 px-4 py-2">
                <div className="min-w-0">
                  <span className="font-medium text-foreground">{w.word}</span>
                  {w.aliases && <span className="text-sm text-muted-foreground"> · høres som {w.aliases}</span>}
                </div>
                <div className="flex shrink-0">
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-11 w-11"
                    aria-label={`Ret ${w.word}`}
                    onClick={() => {
                      setWordId(w.id);
                      setWord(w.word);
                      setAliases(w.aliases);
                    }}
                  >
                    <Pencil aria-hidden="true" />
                  </Button>
                  <Button variant="ghost" size="icon" className="h-11 w-11" aria-label={`Slet ${w.word}`} onClick={() => removeWord(w.id)}>
                    <Trash2 aria-hidden="true" />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Snippets" description="Sig udløseren, så indsættes teksten. Fx 'min signatur' eller 'mit CVR'.">
        <form onSubmit={saveSnippetForm} className="space-y-2">
          <div className="space-y-1">
            <label htmlFor="dict-snippet-phrase" className="text-sm font-medium text-foreground">Udløser</label>
            <Input id="dict-snippet-phrase" value={phrase} onChange={(e) => setPhrase(e.target.value)} placeholder="min signatur" />
          </div>
          <div className="space-y-1">
            <label htmlFor="dict-snippet-text" className="text-sm font-medium text-foreground">Tekst</label>
            <Textarea id="dict-snippet-text" value={snippetText} onChange={(e) => setSnippetText(e.target.value)} rows={3} />
          </div>
          <Button type="submit" disabled={!phrase.trim() || !snippetText.trim()}>
            {snippetId === null ? 'Tilføj snippet' : 'Gem snippet'}
          </Button>
        </form>
        {snippets.length === 0 ? (
          <p className="text-sm text-muted-foreground">Ingen snippets endnu.</p>
        ) : (
          <ul className="divide-y divide-border rounded-lg border border-border">
            {snippets.map((s) => (
              <li key={s.id} className="flex items-start justify-between gap-3 px-4 py-2">
                <div className="min-w-0">
                  <div className="font-medium text-foreground">{s.phrase}</div>
                  <div className="text-sm text-muted-foreground whitespace-pre-wrap line-clamp-3">{s.text}</div>
                </div>
                <div className="flex shrink-0">
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-11 w-11"
                    aria-label={`Ret ${s.phrase}`}
                    onClick={() => {
                      setSnippetId(s.id);
                      setPhrase(s.phrase);
                      setSnippetText(s.text);
                    }}
                  >
                    <Pencil aria-hidden="true" />
                  </Button>
                  <Button variant="ghost" size="icon" className="h-11 w-11" aria-label={`Slet ${s.phrase}`} onClick={() => removeSnippet(s.id)}>
                    <Trash2 aria-hidden="true" />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Historik og privatliv">
        <ToggleRow
          id="dict-history"
          label="Gem historik"
          hint="De seneste 500 dikteringer med tekst, svartid og program"
          checked={settings.historyEnabled}
          onChange={(historyEnabled) => save({ historyEnabled })}
        />
        <ToggleRow
          id="dict-keep-audio"
          label="Gem lyden i 24 timer"
          hint="Optagelsen slettes automatisk efter et døgn. Teksten bliver i historikken."
          checked={settings.keepAudio}
          onChange={(keepAudio) => save({ keepAudio })}
          disabled={!settings.historyEnabled}
        />
        <p className="text-sm text-muted-foreground">
          Med Syv.ai sendes kun lydoptagelsen af det, du dikterer, og ordene i din ordbog. Tastetryk gemmes aldrig, og
          nøglen ligger i Windows Legitimationsstyring, ikke i Tingskrivers filer.
        </p>
      </Section>
    </div>
  );
}
