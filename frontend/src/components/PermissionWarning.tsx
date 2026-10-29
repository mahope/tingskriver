import React from 'react';
import { AlertTriangle, Mic, Speaker, RefreshCw } from 'lucide-react';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { invoke } from '@tauri-apps/api/core';
import { useIsLinux } from '@/hooks/usePlatform';

interface PermissionWarningProps {
  hasMicrophone: boolean;
  hasSystemAudio: boolean;
  onRecheck: () => void;
  isRechecking?: boolean;
}

export function PermissionWarning({
  hasMicrophone,
  hasSystemAudio,
  onRecheck,
  isRechecking = false
}: PermissionWarningProps) {
  const isLinux = useIsLinux();

  // Don't show on Linux - permission handling is not needed
  if (isLinux) {
    return null;
  }

  // Don't show if both permissions are granted
  if (hasMicrophone && hasSystemAudio) {
    return null;
  }

  const isMacOS = navigator.userAgent.includes('Mac');

  const openMicrophoneSettings = async () => {
    if (isMacOS) {
      try {
        await invoke('open_system_settings', { preferencePane: 'Privacy_Microphone' });
      } catch (error) {
        console.error('Failed to open microphone settings:', error);
      }
    }
  };

  const openScreenRecordingSettings = async () => {
    if (isMacOS) {
      try {
        await invoke('open_system_settings', { preferencePane: 'Privacy_ScreenCapture' });
      } catch (error) {
        console.error('Failed to open screen recording settings:', error);
      }
    }
  };

  return (
    <div className="max-w-md mb-4 space-y-3">
      {/* Combined Permission Warning - Show when either permission is missing */}
      {(!hasMicrophone || !hasSystemAudio) && (
        <Alert variant="destructive" className="border-warning/50 bg-warning/10">
          <AlertTriangle className="h-5 w-5 text-warning" />
          <AlertTitle className="text-warning font-semibold">
            <div className="flex items-center gap-2">
              {!hasMicrophone && <Mic className="h-4 w-4" />}
              {!hasSystemAudio && <Speaker className="h-4 w-4" />}
              {!hasMicrophone && !hasSystemAudio ? 'Tilladelser mangler' : !hasMicrophone ? 'Tilladelse til mikrofon mangler' : 'Tilladelse til systemlyd mangler'}
            </div>
          </AlertTitle>
          {/* Action Buttons */}
          <div className="mt-4 flex flex-wrap gap-2">
            {isMacOS && !hasMicrophone && (
              <button
                onClick={openMicrophoneSettings}
                className="inline-flex items-center gap-2 px-4 py-2 min-h-[44px] text-sm font-medium text-warning-foreground bg-warning hover:bg-warning/90 rounded-md transition-colors"
              >
                <Mic className="h-4 w-4" />
                Åbn mikrofonindstillinger
              </button>
            )}
            {isMacOS && !hasSystemAudio && (
              <button
                onClick={openScreenRecordingSettings}
                className="inline-flex items-center gap-2 px-4 py-2 min-h-[44px] text-sm font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition-colors"
              >
                <Speaker className="h-4 w-4" />
                Åbn indstillinger for skærmoptagelse
              </button>
            )}
            <button
              onClick={onRecheck}
              disabled={isRechecking}
              className="inline-flex items-center gap-2 px-4 py-2 min-h-[44px] text-sm font-medium text-warning bg-warning/15 hover:bg-warning/25 rounded-md transition-colors disabled:opacity-50 focus-visible:ring-2 focus-visible:ring-ring"
            >
              <RefreshCw className={`h-4 w-4 ${isRechecking ? 'animate-spin' : ''}`} />
              Tjek igen
            </button>
          </div>
          <AlertDescription className="text-warning mt-2">
            {/* Microphone Warning */}
            {!hasMicrophone && (
              <>
                <p className="mb-3">
                  Tingskriver skal bruge din mikrofon for at optage møder, men ingen mikrofon blev fundet.
                </p>
                <div className="space-y-2 text-sm mb-4">
                  <p className="font-medium">Tjek følgende:</p>
                  <ul className="list-disc list-inside ml-2 space-y-1">
                    <li>Mikrofonen er sluttet til og tændt</li>
                    <li>Tingskriver har tilladelse til mikrofonen i systemindstillingerne</li>
                    <li>Ingen anden app har eneadgang til mikrofonen</li>
                  </ul>
                </div>
              </>
            )}

            {/* System Audio Warning */}
            {!hasSystemAudio && (
              <>
                <p className="mb-3">
                  {hasMicrophone
                    ? 'Systemlyd kan ikke optages. Du kan stadig optage med mikrofonen, men lyd fra computeren kommer ikke med.'
                    : 'Systemlyd kan heller ikke optages.'}
                </p>
                {isMacOS && (
                  <div className="space-y-2 text-sm mb-4">
                    <p className="font-medium">Sådan slår du systemlyd til på macOS:</p>
                    <ul className="list-disc list-inside ml-2 space-y-1">
                      <li>Installér en virtuel lydenhed, fx BlackHole 2ch</li>
                      <li>Giv Tingskriver tilladelse til skærmoptagelse</li>
                      <li>Sæt lydrouting op i Audio MIDI-opsætning</li>
                    </ul>
                  </div>
                )}
              </>
            )}


          </AlertDescription>
        </Alert>
      )}
    </div>
  );
}
