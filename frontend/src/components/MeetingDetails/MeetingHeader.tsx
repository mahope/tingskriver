'use client';

import { useEffect, useRef, useState } from 'react';
import { Pencil } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { formatDate, formatTime } from '@/lib/format';

interface MeetingHeaderProps {
  meetingId: string;
  title: string;
  createdAt?: string;
  onTitleSaved: (title: string) => void;
  /** Handlinger (opsummér, kopiér, gem) under titlen */
  actions?: React.ReactNode;
}

/** Mødets titel (redigerbar), dato og klokkeslæt. */
export function MeetingHeader({ meetingId, title, createdAt, onTitleSaved, actions }: MeetingHeaderProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(title);
  const inputRef = useRef<HTMLInputElement>(null);
  const finishedRef = useRef(false);

  useEffect(() => {
    if (!editing) setDraft(title);
  }, [title, editing]);

  useEffect(() => {
    if (editing) {
      finishedRef.current = false;
      inputRef.current?.select();
    }
  }, [editing]);

  const finish = async (save: boolean) => {
    if (finishedRef.current) return;
    finishedRef.current = true;
    setEditing(false);
    const next = draft.trim();
    if (!save || next === title) {
      setDraft(title);
      return;
    }
    if (!next) {
      toast.error('Mødet skal have et navn');
      setDraft(title);
      return;
    }
    try {
      await invoke('api_save_meeting_title', { meetingId, title: next });
      onTitleSaved(next);
    } catch (error) {
      console.error('Failed to save meeting title:', error);
      toast.error('Navnet kunne ikke gemmes', { description: 'Prøv igen om lidt.' });
      setDraft(title);
    }
  };

  const created = createdAt ? new Date(createdAt) : null;
  const hasDate = created !== null && !Number.isNaN(created.getTime());

  return (
    <header>
      {editing ? (
        <input
          ref={inputRef}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => finish(true)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') finish(true);
            if (e.key === 'Escape') finish(false);
          }}
          aria-label="Mødets navn"
          className="w-full rounded-md border border-input bg-background px-2 py-1 font-serif text-4xl font-semibold leading-tight focus-visible:ring-2 focus-visible:ring-ring"
        />
      ) : (
        <h1 className="group flex items-start gap-2 font-serif text-4xl font-semibold leading-tight text-foreground">
          <span className="min-w-0 break-words">{title}</span>
          <button
            type="button"
            onClick={() => setEditing(true)}
            aria-label="Omdøb mødet"
            title="Omdøb mødet"
            className="mt-1.5 inline-flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md text-muted-foreground opacity-0 transition-opacity hover:bg-accent hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100"
          >
            <Pencil className="h-4 w-4" aria-hidden="true" />
          </button>
        </h1>
      )}
      {hasDate && (
        <p className="mt-2 text-sm text-muted-foreground">
          {formatDate(created)} kl. {formatTime(created)}
        </p>
      )}
      {actions && <div className="mt-4">{actions}</div>}
    </header>
  );
}
