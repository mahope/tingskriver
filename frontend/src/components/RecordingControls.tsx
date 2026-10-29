'use client';

import { invoke } from '@tauri-apps/api/core';
import { appDataDir } from '@tauri-apps/api/path';
import { useCallback, useEffect, useState, useRef } from 'react';
import { Play, Pause, Square, Mic, AlertCircle, X } from 'lucide-react';
import { toast } from 'sonner';
import { ProcessRequest, SummaryResponse } from '@/types/summary';
import { listen } from '@tauri-apps/api/event';
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import Analytics from '@/lib/analytics';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import type { TranscriptionErrorPayload } from '@/services/transcriptService';

interface RecordingControlsProps {
  isRecording: boolean;
  barHeights: string[];
  onRecordingStop: (callApi?: boolean) => void;
  onRecordingStart: () => void;
  onTranscriptReceived: (summary: SummaryResponse) => void;
  onTranscriptionError?: (message: string) => void;
  onStopInitiated?: () => void; // Called immediately when stop button is clicked
  isRecordingDisabled: boolean;
  isParentProcessing: boolean;
  selectedDevices?: {
    micDevice: string | null;
    systemDevice: string | null;
  };
  meetingName?: string;
}

export const RecordingControls: React.FC<RecordingControlsProps> = ({
  isRecording,
  barHeights,
  onRecordingStop,
  onRecordingStart,
  onTranscriptReceived,
  onTranscriptionError,
  onStopInitiated,
  isRecordingDisabled,
  isParentProcessing,
  selectedDevices,
  meetingName,
}) => {
  // Use global recording state context for pause state (syncs with tray operations)
  const recordingState = useRecordingState();
  const isPaused = recordingState.isPaused;
  const isStartingRecording = recordingState.isStartingRecording;

  const [showPlayback, setShowPlayback] = useState(false);
  const [recordingPath, setRecordingPath] = useState<string | null>(null);
  const [transcript, setTranscript] = useState<string>('');
  const [isProcessing, setIsProcessing] = useState(false);
  const [isStarting, setIsStarting] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const [isPausing, setIsPausing] = useState(false);
  const [isResuming, setIsResuming] = useState(false);
  const MIN_RECORDING_DURATION = 2000; // 2 seconds minimum recording time
  const [transcriptionErrors, setTranscriptionErrors] = useState(0);
  const [isValidatingModel, setIsValidatingModel] = useState(false);
  const [speechDetected, setSpeechDetected] = useState(false);
  const [deviceError, setDeviceError] = useState<{ title: string, message: string } | null>(null);

  const currentTime = 0;
  const duration = 0;
  const isPlaying = false;
  const progress = 0;

  const formatTime = (time: number) => {
    const minutes = Math.floor(time / 60);
    const seconds = Math.floor(time % 60);
    return `${minutes}:${seconds.toString().padStart(2, '0')}`;
  };

  useEffect(() => {
    const checkTauri = async () => {
      try {
        const result = await invoke('is_recording');
        console.log('Tauri is initialized and ready, is_recording result:', result);
      } catch (error) {
        console.error('Tauri initialization error:', error);
        toast.error('Optagelsen kunne ikke gøres klar', { description: 'Genstart Tingskriver, og prøv igen.' });
      }
    };
    checkTauri();
  }, []);

  const handleStartRecording = useCallback(async () => {
    if (isStarting || isValidatingModel || isStartingRecording) return;
    console.log('Starting recording...');
    console.log('Selected devices:', selectedDevices);
    console.log('Meeting name:', meetingName);
    console.log('Current isRecording state:', isRecording);

    setShowPlayback(false);
    setTranscript(''); // Clear any previous transcript
    setSpeechDetected(false); // Reset speech detection on new recording

    try {
      // Call the validation callback which will:
      // 1. Check if model is ready
      // 2. Show appropriate toast/modal
      // 3. Call backend if valid
      // 4. Update UI state
      await onRecordingStart();
    } catch (error) {
      console.error('Failed to start recording:', error);
      console.error('Error details:', {
        message: error instanceof Error ? error.message : String(error),
        name: error instanceof Error ? error.name : 'Unknown',
        stack: error instanceof Error ? error.stack : undefined
      });

      // Parse error message to provide user-friendly feedback
      const errorMsg = error instanceof Error ? error.message : String(error);

      // Check for device-related errors
      if (errorMsg.includes('microphone') || errorMsg.includes('mic') || errorMsg.includes('input')) {
        setDeviceError({
          title: 'Mikrofonen er ikke tilgængelig',
          message: 'Tingskriver kan ikke bruge din mikrofon. Tjek at:\n• mikrofonen er sluttet til\n• Tingskriver har adgang til mikrofonen i Windows under Indstillinger > Anonymitet og sikkerhed\n• ingen anden app bruger mikrofonen'
        });
      } else if (errorMsg.includes('system audio') || errorMsg.includes('speaker') || errorMsg.includes('output')) {
        setDeviceError({
          title: 'Systemlyd er ikke tilgængelig',
          message: 'Tingskriver kan ikke optage lyden fra computeren. Tjek at:\n• der er valgt en lydenhed til systemlyd under Indstillinger\n• lydenheden er tændt i Windows'
        });
      } else if (errorMsg.includes('permission')) {
        setDeviceError({
          title: 'Tilladelse mangler',
          message: 'Tingskriver skal have adgang til at optage. Giv adgang til mikrofonen i Windows-indstillingerne, og genstart derefter Tingskriver.'
        });
      } else {
        setDeviceError({
          title: 'Optagelsen kunne ikke starte',
          message: 'Tjek lydenhederne under Indstillinger, og prøv igen.'
        });
      }
    }
  }, [onRecordingStart, isStarting, isValidatingModel, selectedDevices, meetingName, isRecording]);

  const stopRecordingAction = useCallback(async () => {
    console.log('Executing stop recording...');
    try {
      setIsProcessing(true);
      const dataDir = await appDataDir();
      const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
      const savePath = `${dataDir}/recording-${timestamp}.wav`;
      console.log('Saving recording to:', savePath);
      console.log('About to call stop_recording command');
      const result = await invoke('stop_recording', {
        args: {
          save_path: savePath
        }
      });
      console.log('stop_recording command completed successfully:', result);
      setRecordingPath(savePath);
      // setShowPlayback(true);
      setIsProcessing(false);
      // Track successful transcription
      Analytics.trackTranscriptionSuccess();
      onRecordingStop(true);
    } catch (error) {
      console.error('Failed to stop recording:', error);
      if (error instanceof Error) {
        console.error('Error details:', {
          message: error.message,
          name: error.name,
          stack: error.stack,
        });
        if (error.message.includes('No recording in progress')) {
          return;
        }
      } else if (typeof error === 'string' && error.includes('No recording in progress')) {
        return;
      } else if (error && typeof error === 'object' && 'toString' in error) {
        if (error.toString().includes('No recording in progress')) {
          return;
        }
      }
      setIsProcessing(false);
      onRecordingStop(false);
    } finally {
      setIsStopping(false);
    }
  }, [onRecordingStop]);

  const handleStopRecording = useCallback(async () => {
    console.log('handleStopRecording called - isRecording:', isRecording, 'isStarting:', isStarting, 'isStopping:', isStopping, 'isStartingRecording:', isStartingRecording);
    if (!isRecording || isStarting || isStopping || isStartingRecording) {
      console.log('Early return from handleStopRecording due to state check');
      return;
    }

    console.log('Stopping recording...');

    // Notify parent immediately (for UI state updates)
    onStopInitiated?.();

    setIsStopping(true);

    // Immediately trigger the stop action
    await stopRecordingAction();
  }, [isRecording, isStarting, isStopping, isStartingRecording, stopRecordingAction, onStopInitiated]);

  const handlePauseRecording = useCallback(async () => {
    if (!isRecording || isPaused || isPausing) return;

    console.log('Pausing recording...');
    setIsPausing(true);

    try {
      await invoke('pause_recording');
      // isPaused state now managed by RecordingStateContext via events
      console.log('Recording paused successfully');
    } catch (error) {
      console.error('Failed to pause recording:', error);
      toast.error('Optagelsen kunne ikke sættes på pause', { description: 'Prøv igen. Optagelsen kører stadig.' });
    } finally {
      setIsPausing(false);
    }
  }, [isRecording, isPaused, isPausing]);

  const handleResumeRecording = useCallback(async () => {
    if (!isRecording || !isPaused || isResuming) return;

    console.log('Resuming recording...');
    setIsResuming(true);

    try {
      await invoke('resume_recording');
      // isPaused state now managed by RecordingStateContext via events
      console.log('Recording resumed successfully');
    } catch (error) {
      console.error('Failed to resume recording:', error);
      toast.error('Optagelsen kunne ikke genoptages', { description: 'Prøv igen, eller stop og gem optagelsen.' });
    } finally {
      setIsResuming(false);
    }
  }, [isRecording, isPaused, isResuming]);

  useEffect(() => {
    return () => {
      // Cleanup on unmount if needed
    };
  }, []);

  useEffect(() => {
    console.log('Setting up recording event listeners');
    let unsubscribes: (() => void)[] = [];

    const setupListeners = async () => {
      try {
        // Transcript error listener - handles both regular and actionable errors
        const transcriptErrorUnsubscribe = await listen('transcript-error', (event) => {
          console.log('transcript-error event received:', event);
          console.error('Transcription error received:', event.payload);
          const errorMessage = event.payload as string;

          Analytics.trackTranscriptionError(errorMessage);
          console.log('Tracked transcription error:', errorMessage);

          setTranscriptionErrors(prev => {
            const newCount = prev + 1;
            console.log('Transcription error count incremented:', newCount);
            return newCount;
          });
          setIsProcessing(false);
          console.log('Calling onRecordingStop(false) due to transcript error');
          onRecordingStop(false);
          if (onTranscriptionError) {
            onTranscriptionError(errorMessage);
          }
        });

        // Transcription error listener - handles structured error objects with actionable flag
        const transcriptionErrorUnsubscribe = await listen<TranscriptionErrorPayload>('transcription-error', (event) => {
          console.log('transcription-error event received:', event);
          console.error('Transcription error received:', event.payload);

          const errorMessage = event.payload.userMessage || event.payload.error;

          Analytics.trackTranscriptionError(errorMessage);
          console.log('Tracked transcription error:', errorMessage);

          setTranscriptionErrors(prev => {
            const newCount = prev + 1;
            console.log('Transcription error count incremented:', newCount);
            return newCount;
          });
          setIsProcessing(false);

          if (event.payload.phase === 'active') {
            console.log('Calling onRecordingStop(false) due to active transcription error');
            onRecordingStop(false);
          }

          // For actionable errors (like model loading failures), the main page will handle showing the model selector
          // For regular errors, they are handled by useModalState global listener which shows a toast
        });

        // Pause/Resume events are now handled by RecordingStateContext
        // No need for duplicate listeners here

        // Speech detected listener - for UX feedback when VAD detects speech
        const speechDetectedUnsubscribe = await listen('speech-detected', (event) => {
          console.log('speech-detected event received:', event);
          setSpeechDetected(true);
        });

        unsubscribes = [
          transcriptErrorUnsubscribe,
          transcriptionErrorUnsubscribe,
          speechDetectedUnsubscribe
        ];
        console.log('Recording event listeners set up successfully');
      } catch (error) {
        console.error('Failed to set up recording event listeners:', error);
      }
    };

    setupListeners();

    return () => {
      console.log('Cleaning up recording event listeners');
      unsubscribes.forEach(unsubscribe => {
        if (unsubscribe && typeof unsubscribe === 'function') {
          unsubscribe();
        }
      });
    };
  }, [onRecordingStop, onTranscriptionError]);

  const busyStarting = isStarting || isProcessing || isValidatingModel || isStartingRecording;
  const pillButton =
    'inline-flex items-center justify-center gap-2 min-h-[44px] px-5 rounded-full text-sm font-semibold transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-card disabled:opacity-60 disabled:cursor-not-allowed';

  return (
    <div className="flex flex-col items-center gap-2">
      <div className="flex items-center gap-2 bg-card border border-border rounded-full shadow-md px-2.5 py-2">
        {isProcessing && !isParentProcessing ? (
          <div className="flex items-center gap-2 px-3" role="status">
            <div className="animate-spin rounded-full h-5 w-5 border-b-2 border-primary" aria-hidden="true"></div>
            <span className="text-sm text-muted-foreground">Gemmer optagelsen…</span>
          </div>
        ) : !isRecording ? (
          <button
            type="button"
            onClick={() => {
              Analytics.trackButtonClick('start_recording', 'recording_controls');
              handleStartRecording();
            }}
            disabled={busyStarting || isRecordingDisabled}
            className={`${pillButton} bg-primary text-primary-foreground hover:bg-primary/90`}
          >
            {isValidatingModel || isStartingRecording ? (
              <>
                <span className="animate-spin rounded-full h-4 w-4 border-b-2 border-current" aria-hidden="true"></span>
                Gør klar…
              </>
            ) : (
              <>
                <Mic size={18} aria-hidden="true" />
                Start optagelse
              </>
            )}
          </button>
        ) : (
          <>
            <button
              type="button"
              onClick={() => {
                if (isPaused) {
                  Analytics.trackButtonClick('resume_recording', 'recording_controls');
                  handleResumeRecording();
                } else {
                  Analytics.trackButtonClick('pause_recording', 'recording_controls');
                  handlePauseRecording();
                }
              }}
              disabled={isPausing || isResuming || isStopping}
              className={`${pillButton} border border-input bg-card text-foreground hover:bg-accent`}
            >
              {isPaused ? <Play size={16} aria-hidden="true" /> : <Pause size={16} aria-hidden="true" />}
              {isPausing ? 'Sætter på pause…' : isResuming ? 'Genoptager…' : isPaused ? 'Genoptag' : 'Pause'}
            </button>

            <button
              type="button"
              onClick={() => {
                Analytics.trackButtonClick('stop_recording', 'recording_controls');
                handleStopRecording();
              }}
              disabled={isStopping || isPausing || isResuming || isStartingRecording}
              className={`${pillButton} bg-foreground text-background hover:bg-foreground/90`}
            >
              <Square size={14} fill="currentColor" aria-hidden="true" />
              {isStopping ? 'Stopper…' : 'Stop og gem'}
            </button>

            <div className="flex items-center gap-1 mx-3" aria-hidden="true">
              {barHeights.map((height, index) => (
                <div
                  key={index}
                  className={`w-1 rounded-full transition-all duration-200 ${isPaused ? 'bg-warning' : 'bg-recording'}`}
                  style={{
                    height: !isPaused ? height : '4px',
                    opacity: isPaused ? 0.6 : 1,
                  }}
                />
              ))}
            </div>
          </>
        )}
      </div>

      {isValidatingModel && (
        <p className="text-xs text-muted-foreground text-center" role="status">
          Tjekker at talegenkendelsen er klar…
        </p>
      )}

      {deviceError && (
        <Alert variant="destructive" className="mt-2 max-w-md border-destructive/40 bg-destructive/10">
          <AlertCircle className="h-5 w-5 text-destructive" />
          <button
            type="button"
            onClick={() => setDeviceError(null)}
            className="absolute right-2 top-2 h-8 w-8 inline-flex items-center justify-center rounded-md text-destructive hover:bg-destructive/10"
            aria-label="Luk besked"
          >
            <X className="h-4 w-4" />
          </button>
          <AlertTitle className="text-destructive font-semibold mb-2">
            {deviceError.title}
          </AlertTitle>
          <AlertDescription className="text-foreground">
            {deviceError.message.split('\n').map((line, i) => (
              <div key={i} className={i > 0 ? 'ml-2' : ''}>
                {line}
              </div>
            ))}
          </AlertDescription>
        </Alert>
      )}
    </div>
  );
};
