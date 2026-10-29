import React from 'react';

interface ProgressIndicatorProps {
  current: number;
  total: number;
}

export function ProgressIndicator({ current, total }: ProgressIndicatorProps) {
  const steps = Array.from({ length: total }, (_, i) => i + 1);

  return (
    <div className="mb-6 flex flex-col items-center gap-2">
      <p className="text-sm text-muted-foreground" aria-live="polite">
        Trin {current} af {total}
      </p>
      <div className="flex items-center gap-1.5" aria-hidden="true">
        {steps.map((step) => (
          <div
            key={step}
            className={`h-1.5 w-10 rounded-full ${step <= current ? 'bg-primary' : 'bg-muted'}`}
          />
        ))}
      </div>
    </div>
  );
}
