'use client';

/** Below this value a transcript line is marked as uncertain. */
export const LOW_CONFIDENCE_THRESHOLD = 0.4;

interface ConfidenceIndicatorProps {
  confidence: number;
  showIndicator?: boolean;
}

export const ConfidenceIndicator: React.FC<ConfidenceIndicatorProps> = ({
  confidence,
  showIndicator = true,
}) => {
  // Don't render if preference is disabled
  if (!showIndicator) {
    return null;
  }

  // Calm marking: filled dot for confident text, hollow dot when the model was unsure
  const getMarkClass = (conf: number): string => {
    if (conf >= LOW_CONFIDENCE_THRESHOLD) return 'bg-muted-foreground/50';
    return 'border-2 border-muted-foreground bg-transparent';
  };

  // Get descriptive label for accessibility
  const getConfidenceLabel = (conf: number): string => {
    if (conf >= 0.8) return 'Høj sikkerhed';
    if (conf >= 0.7) return 'God sikkerhed';
    if (conf >= 0.4) return 'Middel sikkerhed';
    return 'Lav sikkerhed, tjek teksten';
  };

  const confidencePercent = (confidence * 100).toFixed(0);
  const colorClass = getMarkClass(confidence);
  const label = getConfidenceLabel(confidence);

  return (
    <div
      className="flex items-center gap-1"
      title={`${confidencePercent} % sikkerhed: ${label}`}
      aria-label={`Transskriptionens sikkerhed: ${confidencePercent} procent. ${label}`}
    >
      <div
        className={`w-2 h-2 rounded-full box-border ${colorClass} transition-colors duration-200`}
        role="status"
      />
    </div>
  );
};
