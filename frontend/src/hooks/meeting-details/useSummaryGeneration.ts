import { useState, useCallback, useRef, useEffect } from 'react';
import {
  CancelSummaryResponse,
  MeetingSummary,
  ProcessTranscriptResponse,
  SummaryProcessResponse,
  Transcript,
} from '@/types';
import { ModelConfig } from '@/components/ModelSettingsModal';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { invoke as invokeTauri } from '@tauri-apps/api/core';
import { toast } from 'sonner';

import {
  detectAndCacheSummaryLanguage,
  readMeetingSummaryLanguage,
  readCachedDetectedSummaryLanguage,
} from '@/lib/summary-language-preferences';
import { parseSummaryContent, readSummaryMetadata } from '@/lib/summary-content';

async function resolveSummaryLanguage(
  meetingId: string,
  transcriptTexts: string[]
): Promise<string | null> {
  try {
    const perMeeting = await readMeetingSummaryLanguage(meetingId);
    if (perMeeting.language) return perMeeting.language;
  } catch (err) {
    console.warn('Failed to load meeting summary language:', err);
    toast.warning('Det gemte sprog til opsummering kunne ikke indlæses', {
      description: 'Bruger automatisk sprogvalg denne gang.',
    });
  }

  try {
    const cachedDetected = await readCachedDetectedSummaryLanguage(meetingId);
    if (cachedDetected) return cachedDetected;
  } catch (err) {
    console.warn('Failed to load cached detected summary language:', err);
  }

  try {
    const detection = await detectAndCacheSummaryLanguage(meetingId, transcriptTexts);
    if (detection.reason === 'tie') {
      toast.warning('Transskriptet er tosproget', {
        description: 'Vælg sprog til opsummeringen selv, hvis automatisk sprogvalg vælger forkert.',
      });
    }
    return detection.language;
  } catch (err) {
    console.warn('Failed to detect transcript summary language:', err);
    return null;
  }
}

type SummaryStatus = 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error';

function restoredSummaryStatus(response?: SummaryProcessResponse | null): SummaryStatus {
  if (!response) return 'idle';
  if (response.status === 'pending' || response.status === 'processing') return 'processing';
  if (response.status === 'error' || response.status === 'failed') return 'error';
  if (parseSummaryContent(response.data)) return 'completed';
  if (response.status === 'completed') return 'error';
  return 'idle';
}

interface UseSummaryGenerationProps {
  initialSummary?: SummaryProcessResponse | null;
  meeting: {
    id: string;
    created_at: string;
    is_imported?: boolean;
  };
  transcripts: Transcript[];
  modelConfig: ModelConfig;
  isModelConfigLoading: boolean;
  selectedTemplate: string;
  onMeetingUpdated?: () => Promise<void>;
  updateMeetingTitle: (title: string) => void;
  setAiSummary: (summary: MeetingSummary | null) => void;
  onOpenModelSettings?: () => void;
}

export function useSummaryGeneration({
  initialSummary,
  meeting,
  transcripts,
  modelConfig,
  isModelConfigLoading,
  selectedTemplate,
  onMeetingUpdated,
  updateMeetingTitle,
  setAiSummary,
  onOpenModelSettings,
}: UseSummaryGenerationProps) {
  const restored = initialSummary?.meeting_id === meeting.id ? initialSummary : null;
  const [summaryStatus, setSummaryStatus] = useState<SummaryStatus>(() => restoredSummaryStatus(restored));
  const [summaryError, setSummaryError] = useState<string | null>(() =>
    restoredSummaryStatus(restored) === 'error'
      ? restored?.error || 'Summary generation failed. Please retry.'
      : null,
  );
  const mountedRef = useRef(true);
  const visibleMeetingIdRef = useRef(meeting.id);
  visibleMeetingIdRef.current = meeting.id;
  const generationIdRef = useRef(0);
  const activeProcessIdRef = useRef<string | null>(null);
  const { startSummaryPolling, stopSummaryPolling } = useSidebar();

  const getSummaryStatusMessage = useCallback((status: SummaryStatus) => {
    switch (status) {
      case 'processing':
        return 'Processing transcript...';
      case 'summarizing':
        return 'Generating summary...';
      case 'regenerating':
        return 'Regenerating summary...';
      case 'completed':
        return 'Summary completed';
      case 'error':
        return 'Error generating summary';
      default:
        return '';
    }
  }, []);

  const failGeneration = useCallback(async (
    generationId: number,
    isRegeneration: boolean,
    message: string,
  ) => {
    if (!mountedRef.current || visibleMeetingIdRef.current !== meeting.id || generationId !== generationIdRef.current) {
      return;
    }
    activeProcessIdRef.current = null;
    setSummaryError(message);
    setSummaryStatus('error');
    toast.error(`Opsummeringen kunne ikke ${isRegeneration ? 'genskabes' : 'laves'}`, {
      description: message,
    });
  }, [meeting.id]);

  const handlePollingResult = useCallback(async (
    pollingResult: SummaryProcessResponse,
    generationId: number,
    isRegeneration: boolean,
  ) => {
    if (!mountedRef.current || visibleMeetingIdRef.current !== meeting.id || generationId !== generationIdRef.current) {
      return;
    }
    if (pollingResult.status === 'cancelled') {
      let existing: SummaryProcessResponse;
      try {
        existing = await invokeTauri<SummaryProcessResponse>('api_get_summary', {
          meetingId: meeting.id,
        });
      } catch (error) {
        console.error('Failed to reload summary after cancellation:', error);
        await failGeneration(generationId, isRegeneration,
          'Summary generation was cancelled, but the saved summary could not be reloaded. Please reopen this meeting.');
        return;
      }
      if (!mountedRef.current || visibleMeetingIdRef.current !== meeting.id || generationId !== generationIdRef.current) {
        return;
      }
      const restoredSummary = parseSummaryContent(existing.data);
      setAiSummary(restoredSummary);
      setSummaryStatus(restoredSummary ? 'completed' : 'idle');
      setSummaryError(null);
      activeProcessIdRef.current = null;
      return;
    }

    if (pollingResult.status === 'error' || pollingResult.status === 'failed') {
      const errorMessage = pollingResult.error
        || `Summary ${isRegeneration ? 'regeneration' : 'generation'} failed`;
      if (isRegeneration) {
        let existing: SummaryProcessResponse;
        try {
          existing = await invokeTauri<SummaryProcessResponse>('api_get_summary', {
            meetingId: meeting.id,
          });
        } catch (error) {
          console.error('Failed to reload previous summary after generation failure:', error);
          await failGeneration(generationId, isRegeneration,
            `${errorMessage}. The saved summary could not be reloaded. Please reopen this meeting.`);
          return;
        }
        if (!mountedRef.current || visibleMeetingIdRef.current !== meeting.id || generationId !== generationIdRef.current) {
          return;
        }
        const restoredSummary = parseSummaryContent(existing.data);
        if (restoredSummary) {
          setAiSummary(restoredSummary);
          setSummaryStatus('completed');
          setSummaryError(null);
          toast.error('Opsummeringen kunne ikke genskabes', {
            description: `${errorMessage}. Din tidligere opsummering er gendannet.`,
          });
          activeProcessIdRef.current = null;
          return;
        }
      }
      await failGeneration(generationId, isRegeneration, errorMessage);
      return;
    }

    if (pollingResult.status === 'completed') {
      const summary = parseSummaryContent(pollingResult.data);
      if (!summary) {
        await failGeneration(generationId, isRegeneration,
          'Summary generation completed without visible content. Please retry.',
        );
        return;
      }
      const metadata = readSummaryMetadata(pollingResult.data);
      const meetingName = metadata.meetingName || pollingResult.meetingName;
      if (meetingName) {
        updateMeetingTitle(meetingName);
      }
      setAiSummary(summary);
      setSummaryStatus('completed');
      activeProcessIdRef.current = null;
      setSummaryError(null);
      if (metadata.normalizationFallback) {
        toast.warning('Opsummeringen er lavet med reservevalg', {
          description: 'Normalisering til engelsk mislykkedes, så den oprindelige, rensede opsummering blev beholdt.',
        });
      } else {
        toast.success('Opsummeringen er klar', {
          description: metadata.reasoningStripped
            ? 'Mødets opsummering er klar. Modellens tankegang er filtreret fra noterne.'
            : 'Mødets opsummering er klar',
          duration: 4000,
        });
      }
      if (meetingName && onMeetingUpdated) {
        await onMeetingUpdated();
      }
    }
  }, [failGeneration, meeting.id, onMeetingUpdated, setAiSummary, updateMeetingTitle]);

  // Keep polling attached across ordinary rerenders without retaining stale view callbacks.
  const pollingResultRef = useRef(handlePollingResult);
  pollingResultRef.current = handlePollingResult;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (activeProcessIdRef.current) {
        stopSummaryPolling(meeting.id, activeProcessIdRef.current);
      }
    };
  }, [meeting.id, stopSummaryPolling]);

  useEffect(() => {
    if (!initialSummary || initialSummary.meeting_id !== meeting.id) return;
    const status = restoredSummaryStatus(initialSummary);
    setSummaryStatus(status);
    setSummaryError(status === 'error'
      ? initialSummary.error || 'Summary generation failed. Please retry.'
      : null);
    if (status !== 'processing' || !initialSummary.start) return;

    const generationId = ++generationIdRef.current;
    activeProcessIdRef.current = initialSummary.start;
    const isRegeneration = !!parseSummaryContent(initialSummary.data);
    startSummaryPolling(meeting.id, initialSummary.start, result =>
      pollingResultRef.current(result, generationId, isRegeneration));
  }, [initialSummary, meeting.id, startSummaryPolling]);

  const processSummary = useCallback(async ({
    transcriptText,
    transcriptTexts,
    customPrompt = '',
    isRegeneration = false,
  }: {
    transcriptText: string;
    transcriptTexts?: string[];
    customPrompt?: string;
    isRegeneration?: boolean;
  }) => {
    const generationId = ++generationIdRef.current;
    activeProcessIdRef.current = null;
    setSummaryStatus(isRegeneration ? 'regenerating' : 'processing');
    setSummaryError(null);

    try {
      if (!transcriptText.trim()) {
        await failGeneration(generationId, isRegeneration, 'No transcript text available. Please add some text first.');
        return;
      }

      toast.info(`${isRegeneration ? 'Genskaber' : 'Laver'} opsummering…`, {
        description: `Bruger ${modelConfig.provider}/${modelConfig.model}`,
        duration: 3000,
      });

      const summaryLanguage = await resolveSummaryLanguage(
        meeting.id,
        transcriptTexts?.length ? transcriptTexts : [transcriptText],
      );
      if (!mountedRef.current || visibleMeetingIdRef.current !== meeting.id || generationId !== generationIdRef.current) {
        return;
      }

      const result = meeting.is_imported
        ? await invokeTauri<ProcessTranscriptResponse>('regenerate_summary_command', {
          meetingId: meeting.id, customPrompt, templateId: selectedTemplate, summaryLanguage,
        })
        : await invokeTauri<ProcessTranscriptResponse>('api_process_transcript', {
          text: transcriptText,
          model: modelConfig.provider,
          modelName: modelConfig.model,
          meetingId: meeting.id,
          chunkSize: 40000,
          overlap: 1000,
          customPrompt,
          templateId: selectedTemplate,
          summaryLanguage,
        });
      const processId = result.process_id;
      if (!mountedRef.current || visibleMeetingIdRef.current !== meeting.id) return;
      if (generationId !== generationIdRef.current) {
        await invokeTauri('api_cancel_summary', { meetingId: meeting.id, processId }).catch(() => {});
        return;
      }
      activeProcessIdRef.current = processId;

      startSummaryPolling(meeting.id, processId, result =>
        pollingResultRef.current(result, generationId, isRegeneration));
    } catch (error) {
      await failGeneration(generationId, isRegeneration, error instanceof Error ? error.message : 'Summary generation failed.');
    }
  }, [
    failGeneration,
    meeting.is_imported,
    meeting.id,
    modelConfig,
    onMeetingUpdated,
    selectedTemplate,
    setAiSummary,
    startSummaryPolling,
    updateMeetingTitle,
  ]);

  // Helper function to fetch ALL transcripts for summary generation
  const fetchAllTranscripts = useCallback(async (meetingId: string): Promise<Transcript[]> => {
    try {
      console.log('📊 Fetching all transcripts for meeting:', meetingId);

      // First, get total count by fetching first page
      const firstPage = await invokeTauri('api_get_meeting_transcripts', {
        meetingId,
        limit: 1,
        offset: 0,
      }) as { transcripts: Transcript[]; total_count: number; has_more: boolean };

      const totalCount = firstPage.total_count;
      console.log(`📊 Total transcripts in database: ${totalCount}`);

      if (totalCount === 0) {
        return [];
      }

      // Fetch all transcripts in one call
      const allData = await invokeTauri('api_get_meeting_transcripts', {
        meetingId,
        limit: totalCount,
        offset: 0,
      }) as { transcripts: Transcript[]; total_count: number; has_more: boolean };

      console.log(`✅ Fetched ${allData.transcripts.length} transcripts from database`);
      return allData.transcripts;
    } catch (error) {
      console.error('❌ Error fetching all transcripts:', error);
      toast.error('Transskriptet kunne ikke hentes til opsummeringen');
      return [];
    }
  }, []);

  const buildSummaryTranscriptPayload = useCallback((allTranscripts: Transcript[]) => {
    const formatTime = (seconds: number | undefined, fallbackTimestamp: string): string => {
      if (seconds === undefined) {
        return fallbackTimestamp;
      }
      const totalSecs = Math.floor(seconds);
      return `[${Math.floor(totalSecs / 60).toString().padStart(2, '0')}:${(totalSecs % 60).toString().padStart(2, '0')}]`;
    };

    return {
      transcriptText: allTranscripts
        .map((transcript) => `${formatTime(transcript.audio_start_time, transcript.timestamp)} ${transcript.speaker && !['mic', 'system'].includes(transcript.speaker) ? `${transcript.speaker}: ` : ''}${transcript.text}`)
        .join('\n'),
      transcriptTexts: allTranscripts.map((transcript) => transcript.text),
    };
  }, []);

  const showPreflightError = useCallback((message: string) => {
    setSummaryError(message);
    setSummaryStatus('error');
    toast.error(message);
  }, []);

  const handleGenerateSummary = useCallback(async (customPrompt: string = '') => {
    if (isModelConfigLoading) {
      toast.info('Henter modelindstillinger…');
      return;
    }
    const allTranscripts = await fetchAllTranscripts(meeting.id);
    if (!allTranscripts.length) {
      showPreflightError('Der er intet transskript at opsummere');
      return;
    }

    try {
      if (modelConfig.provider === 'ollama') {
        const models = await invokeTauri<unknown[]>('get_ollama_models', {
          endpoint: modelConfig.ollamaEndpoint || null,
        });
        if (models.length === 0) {
          showPreflightError('Der er ingen Ollama-modeller. Hent gemma3:1b under modelindstillingerne.');
          return;
        }
      }
      if (modelConfig.provider === 'builtin-ai') {
        if (!modelConfig.model) {
          showPreflightError('Der er ikke valgt en indbygget AI-model. Vælg en under indstillingerne.');
          onOpenModelSettings?.();
          return;
        }
        const isReady = await invokeTauri<boolean>('builtin_ai_is_model_ready', {
          modelName: modelConfig.model,
          refresh: true,
        });
        if (!isReady) {
          showPreflightError('Den indbyggede AI-model er ikke klar. Tjek modelindstillingerne.');
          onOpenModelSettings?.();
          return;
        }
      }
    } catch (error) {
      console.error('Failed to validate summary model:', error);
      showPreflightError('Opsummeringsmodellen kunne ikke kontrolleres. Tjek modelindstillingerne.');
      return;
    }

    await processSummary({
      ...buildSummaryTranscriptPayload(allTranscripts),
      customPrompt,
    });
  }, [
    buildSummaryTranscriptPayload,
    fetchAllTranscripts,
    isModelConfigLoading,
    meeting.id,
    modelConfig,
    onOpenModelSettings,
    processSummary,
    showPreflightError,
  ]);

  // Public API: Regenerate summary from the current saved transcript
  const handleRegenerateSummary = useCallback(async () => {
    const allTranscripts = await fetchAllTranscripts(meeting.id);

    if (!allTranscripts.length) {
      console.error('No transcripts available for regeneration');
      toast.error('Der er intet transskript at genskabe opsummeringen ud fra');
      return;
    }

    await processSummary({
      ...buildSummaryTranscriptPayload(allTranscripts),
      isRegeneration: true
    });
  }, [meeting.id, fetchAllTranscripts, buildSummaryTranscriptPayload, processSummary]);

  // Public API: Stop ongoing summary generation
  const handleStopGeneration = useCallback(async () => {
    const generationId = generationIdRef.current;
    const processId = activeProcessIdRef.current;
    if (!processId) {
      generationIdRef.current += 1;
      activeProcessIdRef.current = null;
      setSummaryStatus('idle');
      setSummaryError(null);
      toast.info('Opsummeringen er stoppet', {
        description: 'Du kan lave en ny opsummering, når du vil',
        duration: 3000,
      });
      return;
    }

    try {
      const result = await invokeTauri<CancelSummaryResponse>('api_cancel_summary', {
        meetingId: meeting.id,
        processId,
      });
      if (!mountedRef.current || visibleMeetingIdRef.current !== meeting.id || generationId !== generationIdRef.current) {
        return;
      }
      if (result.cancelled) {
        generationIdRef.current += 1;
        activeProcessIdRef.current = null;
        stopSummaryPolling(meeting.id, processId);
        setSummaryStatus('idle');
        setSummaryError(null);
        toast.info('Opsummeringen er stoppet', {
          description: 'Du kan lave en ny opsummering, når du vil',
          duration: 3000,
        });
      } else if (activeProcessIdRef.current === processId) {
        toast.info('Opsummeringen er næsten færdig', {
          description: 'Venter på det seneste resultat.',
        });
      }
    } catch (error) {
      console.error('Failed to cancel summary generation:', error);
      if (
        generationId === generationIdRef.current
        && activeProcessIdRef.current === processId
      ) {
        toast.error('Opsummeringen kunne ikke stoppes', {
          description: 'Opsummeringen kører stadig. Venter på den seneste status.',
        });
      }
    }
  }, [meeting.id, stopSummaryPolling]);

  return {
    summaryStatus,
    summaryError,
    handleGenerateSummary,
    handleRegenerateSummary,
    handleStopGeneration,
    getSummaryStatusMessage,
  };
}
