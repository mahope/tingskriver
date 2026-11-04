'use client';

import { useCallback, useEffect, useId, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { control, statusLabel, type Destination, type Policy, type Request, type Status } from './model';

export function DeliverySettings({ request = invoke }: { request?: Request }) {
  const prefix = useId();
  const [status, setStatus] = useState<Status | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const [kind, setKind] = useState<Destination['kind']>('folder');
  const refresh = useCallback(async () => setStatus(await request<Status>('delivery_status')), [request]);
  useEffect(() => {
    let active = true;
    const load = async () => {
      try { const next = await request<Status>('delivery_status'); if (active) setStatus(next); }
      catch { if (active) setError('Afleveringsindstillingerne kunne ikke læses. Prøv at opdatere.'); }
    };
    void load();
    const timer = setInterval(() => void load(), 15000);
    return () => { active = false; clearInterval(timer); };
  }, [request]);

  async function action(work: () => Promise<unknown>, done = '') {
    setBusy(true); setError(''); setMessage('');
    try {
      await work(); await refresh(); setMessage(done);
      if (typeof window !== 'undefined') window.dispatchEvent(new Event('delivery-settings-changed'));
    }
    catch { setError('Handlingen mislykkedes. Kontrollér mål, adgang og privatindstillinger, og prøv igen.'); }
    finally { setBusy(false); }
  }

  function field(name: string, label: string, type = 'text', defaultValue?: string) {
    return <label className="block space-y-2" htmlFor={`${prefix}-${name}`}>
      <span>{label}</span>
      <input id={`${prefix}-${name}`} name={name} type={type} required defaultValue={defaultValue}
        autoComplete="off" spellCheck={false} className={control} />
    </label>;
  }

  async function save(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const values = new FormData(form);
    const text = (name: string) => String(values.get(name) ?? '').trim();
    let destination: Destination;
    if (kind === 'folder') destination = { kind, path: text('path') };
    else if (kind === 'webhook') destination = { kind, format: text('format') as 'json' | 'slack' | 'discord' | 'teams' };
    else destination = { kind, host: text('host'), user: text('user'), port: Number(text('port')), key_path: text('key_path'), remote_dir: text('remote_dir') };
    const args = { target: { id: crypto.randomUUID(), name: text('name'), destination }, webhookUrl: kind === 'webhook' ? text('url') : null };
    // Clear tokens from the form even if saving fails. Do not log IPC errors.
    const url = form.elements.namedItem('url') as HTMLInputElement | null;
    if (url) url.value = '';
    await action(async () => { await request('delivery_save_target', args); form.reset(); }, 'Målet er gemt. Intet er sendt.');
  }

  const blocked = !status || status.policy.local_only || status.policy.private;
  function policy(next: Policy) { void action(() => request('delivery_set_policy', { policy: next })); }

  return <section className="space-y-6 border-t border-border pt-8" aria-labelledby={`${prefix}-title`}>
    <h2 id={`${prefix}-title`} className="text-xl font-semibold">Aflevering af referat</h2>
    <p className="max-w-prose text-base text-muted-foreground">Opret et mål, og godkend referatet på mødesiden før aflevering. Mappe og SSH får transskript, referat, noter og en metadatafil. Kanal-webhooks får et afkortet referat.</p>
    {error && <p role="alert" className="text-destructive">{error}</p>}
    <p role="status" className="min-h-6">{busy ? 'Arbejder…' : message}</p>
    {!status ? <Button variant="outline" disabled={busy} onClick={() => void action(refresh)}>Opdatér</Button> : <>
      <fieldset disabled={busy} className="space-y-2">
        <legend className="font-medium">Privatliv</legend>
        <label className="flex min-h-[44px] items-center gap-3"><input type="checkbox" className="h-5 w-5 accent-primary focus-visible:ring-2 focus-visible:ring-ring" checked={status.policy.local_only} onChange={e => policy({ ...status.policy, local_only: e.target.checked })} />Kun lokal. Blokér alle afleveringer, også mapper</label>
        <label className="flex min-h-[44px] items-center gap-3"><input type="checkbox" className="h-5 w-5 accent-primary focus-visible:ring-2 focus-visible:ring-ring" checked={status.policy.private} onChange={e => policy({ ...status.policy, private: e.target.checked })} />Privat tilstand. Sæt afleveringskøen på pause</label>
      </fieldset>
      <div className="space-y-4">
        <h3 className="text-lg font-semibold">Dine mål</h3>
        {status.targets.length === 0 && <p>Du har ingen mål. Opret et nedenfor for at komme i gang.</p>}
        {status.targets.map(target => <div key={target.id} className="flex flex-wrap items-center gap-3 border-b border-border pb-4">
          <span className="min-w-0 flex-1 break-words">{target.name}</span>
          <Button variant="outline" disabled={busy || blocked} onClick={() => void action(() => request('delivery_test', { id: target.id }), 'Testen er lagt i kø uden mødedata. Se resultatet nedenfor.')}>Test mål</Button>
          <Button variant="outline" disabled={busy} onClick={() => {
            if (window.confirm(`Fjern målet ${target.name}? Ventende afleveringer til målet kan ikke sendes.`)) void action(() => request('delivery_delete_target', { id: target.id }));
          }}>Fjern mål</Button>
        </div>)}
      </div>
      <form onSubmit={save} className="max-w-xl space-y-4">
        <fieldset disabled={busy} className="space-y-4">
          <legend className="mb-4 text-lg font-semibold">Tilføj mål</legend>
          {field('name', 'Navn på målet')}
          <label className="block space-y-2" htmlFor={`${prefix}-kind`}><span>Måltype</span><select id={`${prefix}-kind`} className={control} value={kind} onChange={e => setKind(e.target.value as Destination['kind'])}>
            <option value="folder">Mappe</option><option value="webhook">Webhook</option><option value="ssh">SSH med SFTP</option>
          </select></label>
          {kind === 'folder' && field('path', 'Fuld sti til mappe')}
          {kind === 'webhook' && <>
            {field('url', 'Webhook-adresse. Gemmes i systemets nøglelager', 'password')}
            <label className="block space-y-2" htmlFor={`${prefix}-format`}><span>Format</span><select id={`${prefix}-format`} name="format" className={control}>
              <option value="json">Generisk JSON med alle dokumenter</option><option value="slack">Slack</option><option value="discord">Discord</option><option value="teams">Teams-kanal</option>
            </select></label>
          </>}
          {kind === 'ssh' && <>
            {field('host', 'Servernavn')}{field('port', 'Port', 'number', '22')}{field('user', 'Brugernavn')}
            {field('key_path', 'Fuld sti til OpenSSH-nøgle')}{field('remote_dir', 'Uploadmappe på serveren, fx /upload')}
            <p className="text-muted-foreground">Serverens værtsnøgle skal være godkendt på forhånd. Brug en nøgle med adgang kun til upload.</p>
          </>}
          <Button type="submit" disabled={busy}>Gem mål</Button>
        </fieldset>
      </form>
      <div className="space-y-4">
        <h3 className="text-lg font-semibold">Afleveringskø</h3>
        {blocked && <p>Køen er sat på pause af dine privatindstillinger.</p>}
        {status.jobs.length === 0 && <p>Ingen afleveringer endnu.</p>}
        <ul className="space-y-4">{status.jobs.map(job => <li key={job.id} className="space-y-2 border-b border-border pb-4">
          <p className="break-words">{status.targets.find(t => t.id === job.target_id)?.name ?? 'Fjernet mål'}: {statusLabel[job.status] ?? job.status}. Antal forsøg: {job.attempts}.</p>
          {job.error && <p className="text-destructive">{job.error}</p>}
          {job.status === 'pending' && !blocked && job.attempts > 0 && <p>Næste forsøg: {new Date(job.next_attempt * 1000).toLocaleString('da-DK')}</p>}
          {['pending', 'failed'].includes(job.status) && <Button variant="outline" disabled={busy || blocked} onClick={() => void action(() => request('delivery_retry', { id: job.id }))}>Prøv igen</Button>}
        </li>)}</ul>
      </div>
    </>}
  </section>;
}
