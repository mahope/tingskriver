'use client';

import { createContext, useContext, useCallback, useRef, ReactNode } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { useConfig } from './ConfigContext';
import { isSubtitleExtension } from '@/constants/subtitleFormats';
import { toast } from 'sonner';

interface ImportDialogContextType {
  openImportDialog: (filePath?: string | null) => void;
  /** Import a .vtt/.srt transcript; asks for a file when no path is given. */
  importTranscript: (filePath?: string | null) => Promise<void>;
}

interface SubtitleImportOutcome {
  meeting_id: string;
  title: string;
  segments_count: number;
  duration_seconds: number;
  already_imported: boolean;
  summary_error: string | null;
}

const ImportDialogContext = createContext<ImportDialogContextType | null>(null);

export const useImportDialog = () => {
  const ctx = useContext(ImportDialogContext);
  if (!ctx) throw new Error('useImportDialog must be used within ImportDialogProvider');
  return ctx;
};

interface ImportDialogProviderProps {
  children: ReactNode;
  onOpen: (filePath?: string | null) => void;
}

export function ImportDialogProvider({ children, onOpen }: ImportDialogProviderProps) {
  const { betaFeatures } = useConfig();
  const router = useRouter();
  const importingRef = useRef(false);

  const openImportDialog = useCallback((filePath?: string | null) => {
    // Gate: Check beta feature flag before opening dialog
    if (!betaFeatures.importAndRetranscribe) {
      toast.error('Beta-funktionen er slået fra', {
        description: 'Slå "Importér lyd og transskribér igen" til under Indstillinger > Beta for at bruge den.'
      });
      return;
    }

    onOpen(filePath);
  }, [onOpen, betaFeatures]);

  const importTranscript = useCallback(async (filePath?: string | null) => {
    if (importingRef.current) return;
    importingRef.current = true;
    try {
      const path = filePath ?? await invoke<string | null>('select_subtitle_file_command');
      if (!path) return; // cancelled
      const ext = path.split('.').pop()?.toLowerCase();
      if (!ext || !isSubtitleExtension(ext)) {
        toast.error('Kun .vtt- og .srt-filer kan importeres');
        return;
      }
      const outcome = await invoke<SubtitleImportOutcome>('import_subtitle_file_command', { path });
      if (outcome.already_imported) {
        toast.info('Transskriptet er allerede importeret', {
          description: 'Det eksisterende møde er åbnet.',
        });
      } else {
        toast.success('Transskriptet er importeret', {
          description: `${outcome.segments_count} afsnit. Referatet bliver lavet med den valgte udbyder.`,
        });
      }
      if (outcome.summary_error) {
        toast.error('Referatet kunne ikke laves', {
          description: 'Mødet og transskriptet er gemt. Brug "Lav referat igen" på mødet.',
        });
      }
      router.push(`/meeting-details?id=${outcome.meeting_id}`);
    } catch (err: any) {
      const message = typeof err === 'string' ? err : (err?.message || 'Filen kunne ikke importeres');
      toast.error('Transskriptet kunne ikke importeres', { description: message });
    } finally {
      importingRef.current = false;
    }
  }, [router]);

  return (
    <ImportDialogContext.Provider value={{ openImportDialog, importTranscript }}>
      {children}
    </ImportDialogContext.Provider>
  );
}
