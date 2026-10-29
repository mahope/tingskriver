'use client';

import { useId, type ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';

interface MeetingDocumentProps {
  /** Titel, metalinje og handlinger øverst på siden */
  header: ReactNode;
  /** Opsummeringen: dokumentets hovedindhold */
  summary: ReactNode;
  /** Transskriptet, foldet ind under opsummeringen */
  transcript: ReactNode;
  transcriptCount: number;
  transcriptOpen: boolean;
  onTranscriptToggle: (open: boolean) => void;
}

/**
 * Mødesiden som et dokument: opsummeringen øverst, transskriptet foldet ind
 * under. Hele siden ruller som én flade, så teksten læses som et dokument
 * og ikke som et kontrolpanel.
 */
export function MeetingDocument({
  header,
  summary,
  transcript,
  transcriptCount,
  transcriptOpen,
  onTranscriptToggle,
}: MeetingDocumentProps) {
  const panelId = useId();

  return (
    <div className="h-screen overflow-y-auto bg-background custom-scrollbar">
      <article className="mx-auto w-full max-w-[46rem] px-8 pt-10 pb-28">
        {header}

        <div className="mt-6">{summary}</div>

        <section className="mt-12 border-t border-border pt-4" aria-label="Transskript">
          <h2 className="font-sans text-base font-semibold">
            <button
              type="button"
              onClick={() => onTranscriptToggle(!transcriptOpen)}
              aria-expanded={transcriptOpen}
              aria-controls={panelId}
              className="group flex w-full min-h-[44px] items-center gap-2 rounded-md px-1 text-left text-foreground hover:bg-accent/60 focus-visible:ring-2 focus-visible:ring-ring"
            >
              <ChevronRight
                className={`h-4 w-4 text-muted-foreground transition-transform ${transcriptOpen ? 'rotate-90' : ''}`}
                aria-hidden="true"
              />
              Transskript
              <span className="text-sm font-normal text-muted-foreground tabular-nums">
                {transcriptCount === 0
                  ? 'Ingen linjer'
                  : transcriptCount === 1
                    ? '1 linje'
                    : `${transcriptCount.toLocaleString('da-DK')} linjer`}
              </span>
              <span className="ml-auto text-sm font-normal text-muted-foreground">
                {transcriptOpen ? 'Skjul' : 'Vis'}
              </span>
            </button>
          </h2>

          <div id={panelId} hidden={!transcriptOpen} className="mt-3">
            <div className="h-[70vh] overflow-hidden rounded-lg border border-border bg-card">
              {transcript}
            </div>
          </div>
        </section>
      </article>
    </div>
  );
}
