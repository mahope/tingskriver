'use client';

import React from 'react';
import { X, Info, Shield } from 'lucide-react';

interface AnalyticsDataModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirmDisable: () => void;
}

export default function AnalyticsDataModal({ isOpen, onClose, onConfirmDisable }: AnalyticsDataModalProps) {
  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-foreground/50 flex items-center justify-center z-50">
      <div role="dialog" aria-modal="true" aria-labelledby="analytics-modal-title" className="bg-card rounded-lg shadow-xl max-w-2xl w-full mx-4 max-h-[90vh] overflow-y-auto">
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-border">
          <div className="flex items-center gap-3">
            <Shield aria-hidden="true" className="w-6 h-6 text-primary" />
            <h2 id="analytics-modal-title" className="text-xl text-foreground">Hvad brugsstatistik indsamler</h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Luk"
            className="p-2 rounded-md text-muted-foreground hover:text-foreground transition-colors focus-visible:ring-2 ring-ring"
          >
            <X aria-hidden="true" className="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div className="p-6 space-y-6">
          {/* Privacy Notice */}
          <div className="bg-success/10 border border-success/30 rounded-lg p-4">
            <div className="flex items-start gap-3">
              <Info aria-hidden="true" className="w-5 h-5 text-success mt-0.5 flex-shrink-0" />
              <div className="text-sm text-success">
                <p className="font-semibold mb-1">Dit privatliv er beskyttet</p>
                <p>Brugsstatistik er slået fra som standard. Slår du den til, indsamler vi <strong>kun anonyme brugsdata</strong>. Vi indsamler aldrig mødeindhold, navne, filstier eller personoplysninger.</p>
              </div>
            </div>
          </div>

          {/* Data Categories */}
          <div className="space-y-4">
            <h3 className="text-lg font-semibold text-foreground">Data, der indsamles, når statistik er slået til</h3>

            {/* Model Preferences */}
            <div className="border border-border rounded-lg p-4">
              <h4 className="font-semibold text-foreground mb-2">1. Valg af modeller</h4>
              <ul className="text-sm text-foreground space-y-1 ml-4">
                <li>• Transskriptionsmodel (fx "Whisper large-v3", "Parakeet")</li>
                <li>• Opsummeringsmodel (fx "Llama 3.2", "Claude Sonnet")</li>
                <li>• Modeludbyder (fx "Lokal", "Ollama", "OpenRouter")</li>
              </ul>
              <p className="text-xs text-muted-foreground mt-2 italic">Hjælper os med at forstå, hvilke modeller der bruges</p>
            </div>

            {/* Meeting Metrics */}
            <div className="border border-border rounded-lg p-4">
              <h4 className="font-semibold text-foreground mb-2">2. Anonyme måltal for møder</h4>
              <ul className="text-sm text-foreground space-y-1 ml-4">
                <li>• Optagelsens længde (fx "125 sekunder")</li>
                <li>• Pausens længde (fx "5 sekunder")</li>
                <li>• Antal transskriptsegmenter</li>
                <li>• Antal behandlede lydstykker</li>
              </ul>
              <p className="text-xs text-muted-foreground mt-2 italic">Hjælper os med at forbedre ydeevnen og forstå, hvordan appen bruges</p>
            </div>

            {/* Device Types */}
            <div className="border border-border rounded-lg p-4">
              <h4 className="font-semibold text-foreground mb-2">3. Enhedstyper (ikke navne)</h4>
              <ul className="text-sm text-foreground space-y-1 ml-4">
                <li>• Mikrofontype: "Bluetooth", "Kablet" eller "Ukendt"</li>
                <li>• Systemlydtype: "Bluetooth", "Kablet" eller "Ukendt"</li>
              </ul>
              <p className="text-xs text-muted-foreground mt-2 italic">Hjælper os med at forbedre kompatibiliteten. Vi indsamler ikke enhedernes navne</p>
            </div>

            {/* Usage Patterns */}
            <div className="border border-border rounded-lg p-4">
              <h4 className="font-semibold text-foreground mb-2">4. Brug af appen</h4>
              <ul className="text-sm text-foreground space-y-1 ml-4">
                <li>• At appen startes og lukkes</li>
                <li>• Sessionens længde</li>
                <li>• Brug af funktioner (fx "indstillinger ændret")</li>
                <li>• Fejl, der opstår (hjælper os med at rette dem)</li>
              </ul>
              <p className="text-xs text-muted-foreground mt-2 italic">Hjælper os med at gøre appen bedre at bruge</p>
            </div>

            {/* Platform Info */}
            <div className="border border-border rounded-lg p-4">
              <h4 className="font-semibold text-foreground mb-2">5. Platform</h4>
              <ul className="text-sm text-foreground space-y-1 ml-4">
                <li>• Styresystem (fx "macOS", "Windows")</li>
                <li>• Appens version (indgår automatisk i alle hændelser)</li>
                <li>• Arkitektur (fx "x86_64", "aarch64")</li>
              </ul>
              <p className="text-xs text-muted-foreground mt-2 italic">Hjælper os med at prioritere, hvilke platforme vi understøtter</p>
            </div>
          </div>

          {/* What We DON'T Collect */}
          <div className="bg-destructive/10 border border-destructive/30 rounded-lg p-4">
            <h4 className="font-semibold text-destructive mb-2">Det indsamler vi ikke</h4>
            <ul className="text-sm text-destructive space-y-1 ml-4">
              <li>• Mødenavne eller titler</li>
              <li>• Filnavne, filstier eller mødemapper</li>
              <li>• Transskripter eller mødeindhold</li>
              <li>• Lydoptagelser</li>
              <li>• Enhedsnavne (kun typer: Bluetooth/Kablet)</li>
              <li>• Personoplysninger</li>
              <li>• Data, der kan identificere dig</li>
            </ul>
          </div>

          {/* Example Event */}
          <div className="bg-background border border-border rounded-lg p-4">
            <h4 className="font-semibold text-foreground mb-2">Eksempel på en hændelse</h4>
            <pre className="text-xs text-foreground overflow-x-auto">
              {`{
  "event": "meeting_ended",
  "app_version": "0.4.1",
  "transcription_provider": "parakeet",
  "transcription_model": "parakeet-tdt-0.6b-v3-int8",
  "summary_provider": "ollama",
  "summary_model": "llama3.2:latest",
  "total_duration_seconds": "125.5",
  "microphone_device_type": "Wired",
  "system_audio_device_type": "Bluetooth",
  "chunks_processed": "150",
  "had_fatal_error": "false"
}`}
            </pre>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between gap-4 p-6 border-t border-border bg-background">
          <button
            type="button"
            onClick={onClose}
            className="min-h-[44px] px-4 py-2 text-foreground bg-card border border-input rounded-md hover:bg-accent transition-colors focus-visible:ring-2 ring-ring"
          >
            Behold statistik slået til
          </button>
          <button
            type="button"
            onClick={onConfirmDisable}
            className="min-h-[44px] px-4 py-2 focus-visible:ring-2 ring-ring text-destructive-foreground bg-destructive rounded-md hover:bg-destructive/90 transition-colors"
          >
            Slå statistik fra
          </button>
        </div>
      </div>
    </div>
  );
}
