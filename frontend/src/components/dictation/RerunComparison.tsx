'use client';

import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';

type Comparison = { historyId: number; before: string; after: string; changed: boolean };

export function RerunComparison() {
  const [comparison, setComparison] = useState<Comparison | null>(null);

  useEffect(() => {
    let disposed = false;
    const unlisten = listen<Comparison>('dictation-rerun-comparison', ({ payload }) => {
      if (disposed) return;
      setComparison(payload);
      const window = getCurrentWindow();
      void window.show().then(() => window.unminimize()).then(() => window.setFocus())
        .catch(() => { console.warn('Could not focus the comparison window'); });
    });
    return () => {
      disposed = true;
      void unlisten.then(stop => stop());
    };
  }, []);

  return (
    <Dialog open={comparison !== null} onOpenChange={open => { if (!open) setComparison(null); }}>
      <DialogContent className="max-h-[85vh] overflow-y-auto">
        <DialogTitle>Kør sidste igen</DialogTitle>
        <DialogDescription>{comparison?.changed ? 'Teksten er ændret.' : 'Teksten er uændret.'}</DialogDescription>
        <h3 className="font-semibold">Før</h3>
        <p className="whitespace-pre-wrap break-words">{comparison?.before}</p>
        <h3 className="font-semibold">Efter</h3>
        <p className="whitespace-pre-wrap break-words">{comparison?.after}</p>
      </DialogContent>
    </Dialog>
  );
}
