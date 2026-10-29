import React from 'react';
import { Upload } from 'lucide-react';
import { getAudioFormatsDisplayList } from '@/constants/audioFormats';

interface ImportDropOverlayProps {
  visible: boolean;
}

export function ImportDropOverlay({ visible }: ImportDropOverlayProps) {
  if (!visible) return null;

  return (
    <div
      className="fixed inset-0 z-50 bg-foreground/60
                 flex items-center justify-center pointer-events-none
                 transition-opacity duration-200"
    >
      <div className="border-2 border-dashed border-primary rounded-2xl
                      p-12 text-center bg-card
                      transform scale-100 transition-transform">
        <Upload className="h-16 w-16 text-primary mx-auto mb-4" />
        <p className="text-xl font-medium text-foreground">Slip lydfilen for at importere den</p>
        <p className="text-sm text-muted-foreground mt-2">{getAudioFormatsDisplayList()}</p>
      </div>
    </div>
  );
}
