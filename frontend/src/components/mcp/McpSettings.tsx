'use client';

import { useEffect, useId, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';

type Request = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type Access = 'all' | 'marked' | 'metadata' | 'off';
type Status = { access: Access; claude: string; codex: string };
const control = 'min-h-11 w-full rounded-md border border-input bg-background px-3 text-base text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring';
const copyText = (text: string) => navigator.clipboard.writeText(text);

export function McpSettings({ request = invoke, copy = copyText }: { request?: Request; copy?: (text: string) => Promise<void> }) {
  const id = useId();
  const [status, setStatus] = useState<Status | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  useEffect(() => {
    let active = true;
    request<Status>('mcp_status').then(value => { if (active) setStatus(value); })
      .catch(() => { if (active) setError('MCP-indstillinger kunne ikke læses. Åbn indstillingerne igen for at prøve igen.'); });
    return () => { active = false; };
  }, [request]);

  async function save(access: Access) {
    setBusy(true); setError(''); setMessage('');
    try {
      await request('mcp_set_access', { access });
      setStatus(current => current ? { ...current, access } : null);
      setMessage('Adgangsniveauet er gemt.');
    } catch { setError('Adgangsniveauet kunne ikke gemmes. Prøv igen.'); }
    finally { setBusy(false); }
  }
  async function copySetup(text: string) {
    setError(''); setMessage('');
    try { await copy(text); setMessage('Opsætningen er kopieret.'); }
    catch { setError('Opsætningen kunne ikke kopieres. Markér og kopiér teksten nedenfor.'); }
  }
  return <section aria-labelledby={`${id}-heading`} className="space-y-4 border-t border-border py-8 text-base">
    <h3 id={`${id}-heading`} className="font-serif text-xl font-semibold">AI-værktøjer (MCP)</h3>
    <p className="max-w-prose text-muted-foreground">Giv Claude Code eller Codex adgang til dine lokale møder. Data, du deler med et AI-værktøj, forlader computeren via det værktøjs udbyder. Kontrollér udbyderens datavilkår først.</p>
    <label htmlFor={`${id}-access`} className="block space-y-2">
      <span>Adgangsniveau</span>
      <select id={`${id}-access`} className={control} value={status?.access ?? 'marked'} disabled={!status || busy} onChange={event => save(event.target.value as Access)}>
        <option value="marked">Kun markerede møder</option>
        <option value="metadata">Kun metadata</option>
        <option value="all">Alt</option>
        <option value="off">Fra</option>
      </select>
    </label>
    {!status && !error && <p role="status">Henter MCP-indstillinger…</p>}
    <p className="max-w-prose text-muted-foreground">Ved Kun markerede møder deles nye møder ikke automatisk. Markér dem på mødesiden. Kun metadata viser titler, datoer, varighed og opgaver. Dikteringer og optagelseshandlinger kræver Alt. Andre handlinger kræver Alt eller et markeret møde og en åben app.</p>
    {status && <div className="space-y-4">
      {(['claude', 'codex'] as const).map(client => <div key={client} className="space-y-2">
        <Button type="button" variant="outline" className="min-h-11" onClick={() => copySetup(status[client])}>Kopiér opsætning til {client === 'claude' ? 'Claude Code' : 'Codex'}</Button>
        <pre tabIndex={0} aria-label={`Opsætning til ${client === 'claude' ? 'Claude Code' : 'Codex'}`} className="max-w-full overflow-x-auto rounded-md border border-border p-4 text-sm focus-visible:ring-2 focus-visible:ring-ring"><code>{status[client]}</code></pre>
      </div>)}
    </div>}
    {error && <p role="alert" className="text-destructive">{error}</p>}
    <p role="status" aria-live="polite">{message}</p>
  </section>;
}

export function MeetingSharing({ id, request = invoke }: { id: string; request?: Request }) {
  const [shared, setShared] = useState(false);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    setReady(false); setShared(false); setError('');
    request<boolean>('mcp_meeting_shared', { id }).then(value => { if (active) { setShared(value); setReady(true); } })
      .catch(() => { if (active) setError('Deling kunne ikke læses. Åbn mødet igen for at prøve igen.'); });
    return () => { active = false; };
  }, [id, request]);
  async function save(next: boolean) {
    setBusy(true); setError('');
    try { await request('mcp_set_meeting_shared', { id, shared: next }); setShared(next); }
    catch { setError('Deling kunne ikke gemmes. Prøv igen.'); }
    finally { setBusy(false); }
  }
  return <div className="mt-4 text-base">
    <label className="inline-flex min-h-11 items-center gap-3">
      <input type="checkbox" className="h-5 w-5 accent-primary focus-visible:ring-2 focus-visible:ring-ring" checked={shared} disabled={!ready || busy} onChange={event => save(event.target.checked)} />
      Må deles med AI-værktøjer
    </label>
    {error && <p role="alert" className="text-destructive">{error}</p>}
  </div>;
}
