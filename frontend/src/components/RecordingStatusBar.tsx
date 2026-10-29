'use client';

import { motion } from 'framer-motion';
import { AlertTriangle, Info, X } from 'lucide-react';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useConfig } from '@/contexts/ConfigContext';
import { useEffect, useState } from 'react';

interface RecordingStatusBarProps {
  isPaused?: boolean;
}

function formatClock(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  const mm = mins.toString().padStart(2, '0');
  const ss = secs.toString().padStart(2, '0');
  return hours > 0 ? `${hours}:${mm}:${ss}` : `${mm}:${ss}`;
}

/**
 * Rolig status for en igangværende optagelse: tilstand, tid, aktive enheder
 * og en besked, når en enhed er skiftet eller faldet ud.
 */
export const RecordingStatusBar: React.FC<RecordingStatusBarProps> = ({ isPaused = false }) => {
  // Varighed i sekunder fra backend (opdateres hvert halve sekund)
  const { activeDuration, activeMicName, deviceNotice, dismissDeviceNotice } = useRecordingState();
  const { selectedDevices } = useConfig();
  const [displaySeconds, setDisplaySeconds] = useState(0);

  useEffect(() => {
    if (activeDuration !== null) {
      setDisplaySeconds(Math.floor(activeDuration));
    }
  }, [activeDuration]);

  const micLabel = deviceNotice?.kind === 'lost'
    ? 'ingen (kun systemlyd)'
    : activeMicName ?? selectedDevices.micDevice ?? 'Standardmikrofon';
  const systemLabel = selectedDevices.systemDevice ?? 'Standard for systemlyd';
  const isWarning = deviceNotice?.kind === 'failed' || deviceNotice?.kind === 'lost';

  return (
    <motion.div
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -6 }}
      transition={{ duration: 0.2 }}
      className="mb-3 rounded-lg border border-border bg-background px-4 py-3"
    >
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1" role="status" aria-live="polite">
        <div className="flex items-center gap-2">
          <span
            className={`h-2.5 w-2.5 rounded-full ${isPaused ? 'bg-warning' : 'bg-recording motion-safe:animate-pulse'}`}
            aria-hidden="true"
          />
          <span className="text-sm font-semibold text-foreground">{isPaused ? 'På pause' : 'Optager'}</span>
          <span className="text-sm tabular-nums text-muted-foreground">{formatClock(displaySeconds)}</span>
        </div>
        <p className="text-sm text-muted-foreground min-w-0 truncate">
          Mikrofon: <span className="text-foreground">{micLabel}</span>
          <span aria-hidden="true">  ·  </span>
          Systemlyd: <span className="text-foreground">{systemLabel}</span>
        </p>
      </div>

      {deviceNotice && (
        <div
          className={`mt-3 flex items-start gap-2 rounded-md border px-3 py-2 text-sm ${
            isWarning ? 'border-warning/40 bg-warning/10 text-foreground' : 'border-primary/30 bg-primary/10 text-foreground'
          }`}
          role="status"
        >
          {isWarning ? (
            <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0 text-warning" aria-hidden="true" />
          ) : (
            <Info className="mt-0.5 h-4 w-4 flex-shrink-0 text-primary" aria-hidden="true" />
          )}
          <p className="flex-1">{deviceNotice.message}</p>
          <button
            type="button"
            onClick={dismissDeviceNotice}
            className="-my-1 -mr-1 inline-flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
            aria-label="Luk beskeden"
          >
            <X className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
      )}
    </motion.div>
  );
};
