'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { control, type Request, type Status } from './model';

export function DeliveryApproval({ meetingId, getSummary, disabled, request = invoke }: {
  meetingId: string; getSummary: () => Promise<string>; disabled: boolean; request?: Request;
}) {
  const [status, setStatus] = useState<Status | null>(null);
  const [target, setTarget] = useState('');
  const [busy, setBusy] = useState(false);
  const [consent, setConsent] = useState(false);
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    setConsent(false); setMessage(''); setError('');
    request<Status>('delivery_status').then(s => { if (active) { setStatus(s); setTarget(s.targets[0]?.id ?? ''); } })
      .catch(() => { if (active) setError('Afleveringsmål kunne ikke læses. Kontrollér indstillingerne.'); });
    return () => { active = false; };
  }, [meetingId, request]);

  async function approve() {
    setBusy(true); setMessage(''); setError('');
    try {
      const summary = await getSummary();
      await request('delivery_approve', { targetId: target, meetingId, summary, private: !consent });
      setMessage('Referatet er godkendt og lagt i afleveringskøen. Se resultatet i Indstillinger.');
    } catch { setError('Referatet kunne ikke afleveres. Kontrollér mål og privatindstillinger, og prøv igen.'); }
    finally { setBusy(false); }
  }

  if (status?.targets.length === 0) return <p className="mt-4 text-muted-foreground">Du kan oprette et afleveringsmål i Indstillinger.</p>;
  return <div className="mt-4 space-y-3 border-t border-border pt-4">
    <label className="block space-y-2" htmlFor={`delivery-target-${meetingId}`}><span>Aflever referatet til</span>
      <select id={`delivery-target-${meetingId}`} value={target} onChange={e => setTarget(e.target.value)} className={control} disabled={busy}>
        {status?.targets.map(t => <option key={t.id} value={t.id}>{t.name}</option>)}
      </select>
    </label>
    <label className="flex min-h-[44px] items-center gap-3"><input type="checkbox" checked={consent} onChange={e => setConsent(e.target.checked)} className="h-5 w-5 accent-primary focus-visible:ring-2 focus-visible:ring-ring" disabled={busy} />Mødet må afleveres til det valgte mål</label>
    {status && (status.policy.local_only || status.policy.private) && <p>Aflevering er blokeret af dine privatindstillinger.</p>}
    {disabled && <p>Gem dine ændringer, før du godkender referatet.</p>}
    <Button disabled={busy || disabled || !target || !consent || !status || status.policy.local_only || status.policy.private} onClick={() => void approve()}>Godkend og aflever referat</Button>
    <p role="status">{busy ? 'Lægger i kø…' : message}</p>
    {error && <p role="alert" className="text-destructive">{error}</p>}
  </div>;
}
