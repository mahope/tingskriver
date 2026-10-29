import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { PermissionWarning } from '@/components/PermissionWarning';
import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Copy, GlobeIcon } from 'lucide-react';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useConfig } from '@/contexts/ConfigContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { ModalType } from '@/hooks/useModalState';
import { useIsLinux } from '@/hooks/usePlatform';
import { useMemo } from 'react';

/**
 * TranscriptPanel Component
 *
 * Displays transcript content with controls for copying and language settings.
 * Uses TranscriptContext, ConfigContext, and RecordingStateContext internally.
 */

interface TranscriptPanelProps {
  // indicates stop-processing state for transcripts; derived from backend statuses.
  isProcessingStop: boolean;
  isStopping: boolean;
  showModal: (name: ModalType, message?: string) => void;
}

export function TranscriptPanel({
  isProcessingStop,
  isStopping,
  showModal
}: TranscriptPanelProps) {
  // Contexts
  const { transcripts, transcriptContainerRef, copyTranscript } = useTranscripts();
  const { transcriptModelConfig } = useConfig();
  const { isRecording, isPaused } = useRecordingState();
  const { checkPermissions, isChecking, hasSystemAudio, hasMicrophone } = usePermissionCheck();
  const isLinux = useIsLinux();

  // Convert transcripts to segments for virtualized view
  const segments = useMemo(() =>
    transcripts.map(t => ({
      id: t.id,
      timestamp: t.audio_start_time ?? 0,
      endTime: t.audio_end_time,
      text: t.text,
      confidence: t.confidence,
    })),
    [transcripts]
  );

  const isIdle = !isRecording && !isProcessingStop && !isStopping && transcripts.length === 0;
  const showToolbar = transcripts.length > 0 || transcriptModelConfig.provider === "localWhisper";

  return (
    <div ref={transcriptContainerRef} className="w-full bg-background flex flex-col overflow-y-auto">
      {/* Tilladelser */}
      {!isRecording && !isChecking && !isLinux && (
        <div className="flex justify-center px-6 pt-4">
          <PermissionWarning
            hasMicrophone={hasMicrophone}
            hasSystemAudio={hasSystemAudio}
            onRecheck={checkPermissions}
            isRechecking={isChecking}
          />
        </div>
      )}

      {isIdle ? (
        <div className="flex flex-1 items-center justify-center px-8 pb-32">
          <div className="max-w-md text-center">
            <h1 className="font-serif text-3xl font-semibold text-foreground">Klar til dit næste møde</h1>
            <p className="mt-3 text-base text-muted-foreground">
              Tingskriver optager og skriver referat på din computer. Lyden bliver her, og du kan læse noterne bagefter.
            </p>
            <p className="mt-2 text-sm text-muted-foreground">
              Tryk på "Start optagelse" nedenfor. Transskriptionen vises her, mens du optager.
            </p>
          </div>
        </div>
      ) : (
        <div className="pb-32 pt-6">
          <div className="mx-auto w-full max-w-[44rem] px-6">
            {showToolbar && (
              <div className="mb-2 flex justify-end">
                <ButtonGroup>
                  {transcripts?.length > 0 && (
                    <Button variant="outline" size="sm" onClick={copyTranscript} title="Kopiér transskriptionen">
                      <Copy />
                      <span className="hidden md:inline">Kopiér</span>
                    </Button>
                  )}
                  {transcriptModelConfig.provider === "localWhisper" && (
                    <Button variant="outline" size="sm" onClick={() => showModal('languageSettings')} title="Vælg sprog">
                      <GlobeIcon />
                      <span className="hidden md:inline">Sprog</span>
                    </Button>
                  )}
                </ButtonGroup>
              </div>
            )}
            <VirtualizedTranscriptView
              segments={segments}
              isRecording={isRecording}
              isPaused={isPaused}
              isProcessing={isProcessingStop}
              isStopping={isStopping}
              enableStreaming={isRecording}
              showConfidence={true}
            />
          </div>
        </div>
      )}
    </div>
  );
}
