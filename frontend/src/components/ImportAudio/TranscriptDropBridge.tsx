'use client';

import { useEffect } from 'react';
import { useImportDialog } from '@/contexts/ImportDialogContext';

export const IMPORT_TRANSCRIPT_EVENT = 'import-transcript-file';

/**
 * Receives dropped .vtt/.srt files from the layout (which sits outside the
 * import context) and starts the import.
 */
export function TranscriptDropBridge() {
  const { importTranscript } = useImportDialog();

  useEffect(() => {
    const handler = (event: Event) => {
      const path = (event as CustomEvent<string>).detail;
      if (path) void importTranscript(path);
    };
    window.addEventListener(IMPORT_TRANSCRIPT_EVENT, handler);
    return () => window.removeEventListener(IMPORT_TRANSCRIPT_EVENT, handler);
  }, [importTranscript]);

  return null;
}
