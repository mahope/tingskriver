import { ModelConfig } from "@/components/ModelSettingsModal";
import { PreferenceSettings } from "@/components/PreferenceSettings";
import { DeviceSelection } from "@/components/DeviceSelection";
import { LanguageSelection } from "@/components/LanguageSelection";
import { TranscriptSettings } from "@/components/TranscriptSettings";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { toast } from "sonner";
import { useEffect } from "react";
import { X } from "lucide-react";
import { useConfig } from "@/contexts/ConfigContext";
import { useRecordingState } from "@/contexts/RecordingStateContext";

type modalType = "modelSettings" | "deviceSettings" | "languageSettings" | "modelSelector" | "errorAlert" | "chunkDropWarning";

/**
 * SettingsModals Component
 *
 * All settings modals consolidated into a single component.
 * Uses ConfigContext and RecordingStateContext internally - no prop drilling needed!
 */

interface SettingsModalsProps {
  modals: {
    modelSettings: boolean;
    deviceSettings: boolean;
    languageSettings: boolean;
    modelSelector: boolean;
    errorAlert: boolean;
    chunkDropWarning: boolean;
  };
  messages: {
    errorAlert: string;
    chunkDropWarning: string;
    modelSelector: string;
  };
  onClose: (name: modalType) => void;
}

export function SettingsModals({
  modals,
  messages,
  onClose,
}: SettingsModalsProps) {
  // Contexts
  const {
    modelConfig,
    setModelConfig,
    models,
    modelOptions,
    error,
    selectedDevices,
    setSelectedDevices,
    selectedLanguage,
    setSelectedLanguage,
    transcriptModelConfig,
    setTranscriptModelConfig,
    showConfidenceIndicator,
    toggleConfidenceIndicator,
  } = useConfig();

  const { isRecording } = useRecordingState();

  return <>
    {/* Indstillinger (ældre dialog) */}
    {modals.modelSettings && (
      <ModalShell title="Indstillinger" onClose={() => onClose("modelSettings")} wide
        footer={<PrimaryButton onClick={() => onClose('modelSettings')}>Færdig</PrimaryButton>}>
        <div className="space-y-8">
          <PreferenceSettings />

          <div className="border-t border-border pt-8">
            <h4 className="font-serif text-lg font-semibold text-foreground mb-4">AI-model til opsummeringer</h4>
            <div className="space-y-4">
              <div>
                <label htmlFor="summary-provider" className="block text-sm font-medium text-foreground mb-1">
                  Model til opsummering
                </label>
                <div className="flex gap-2">
                  <select
                    id="summary-provider"
                    className="min-h-[44px] px-3 text-sm bg-background border border-input rounded-md focus-visible:ring-2 focus-visible:ring-ring"
                    value={modelConfig.provider}
                    onChange={(e) => {
                      const provider = e.target.value as ModelConfig['provider'];
                      setModelConfig({
                        ...modelConfig,
                        provider,
                        model: modelOptions[provider][0]
                      });
                    }}
                  >
                    <option value="builtin-ai">Indbygget AI</option>
                    <option value="claude">Claude</option>
                    <option value="groq">Groq</option>
                    <option value="ollama">Ollama</option>
                    <option value="openrouter">OpenRouter</option>
                    <option value="openai">OpenAI</option>
                  </select>

                  <select
                    aria-label="Model"
                    className="flex-1 min-h-[44px] px-3 text-sm bg-background border border-input rounded-md focus-visible:ring-2 focus-visible:ring-ring"
                    value={modelConfig.model}
                    onChange={(e) => setModelConfig((prev: ModelConfig) => ({ ...prev, model: e.target.value }))}
                  >
                    {modelOptions[modelConfig.provider].map((model: string) => (
                      <option key={model} value={model}>
                        {model}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
              {modelConfig.provider === 'ollama' && (
                <div>
                  <h4 className="text-base font-semibold mb-3">Ollama-modeller på denne computer</h4>
                  {error && (
                    <div role="alert" className="bg-destructive/10 border border-destructive/40 text-destructive px-4 py-3 rounded-md mb-4">
                      {error}
                    </div>
                  )}
                  <div className="grid gap-2 max-h-[400px] overflow-y-auto pr-2">
                    {models.map((model) => {
                      const selected = modelConfig.model === model.name;
                      return (
                        <button
                          type="button"
                          key={model.id}
                          aria-pressed={selected}
                          className={`text-left bg-card border p-4 rounded-lg transition-colors focus-visible:ring-2 focus-visible:ring-ring ${selected ? 'border-primary bg-primary/10' : 'border-border hover:bg-accent'}`}
                          onClick={() => setModelConfig((prev: ModelConfig) => ({ ...prev, model: model.name }))}
                        >
                          <span className="block font-semibold">{model.name}</span>
                          <span className="block text-sm text-muted-foreground">Størrelse: {model.size}</span>
                          <span className="block text-sm text-muted-foreground">Senest ændret: {model.modified}</span>
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      </ModalShell>
    )}

    {modals.deviceSettings && (
      <ModalShell title="Lydenheder" onClose={() => onClose('deviceSettings')}
        footer={
          <PrimaryButton
            onClick={() => {
              const micDevice = selectedDevices.micDevice || 'Standard';
              const systemDevice = selectedDevices.systemDevice || 'Standard';
              toast.success("Lydenhederne er valgt", {
                description: `Mikrofon: ${micDevice}. Systemlyd: ${systemDevice}.`
              });
              onClose('deviceSettings');
            }}
          >
            Færdig
          </PrimaryButton>
        }>
        <DeviceSelection
          selectedDevices={selectedDevices}
          onDeviceChange={setSelectedDevices}
          disabled={isRecording}
        />
      </ModalShell>
    )}

    {modals.languageSettings && (
      <ModalShell title="Sprog" onClose={() => onClose('languageSettings')}
        footer={<PrimaryButton onClick={() => onClose('languageSettings')}>Færdig</PrimaryButton>}>
        <LanguageSelection
          selectedLanguage={selectedLanguage}
          onLanguageChange={setSelectedLanguage}
          disabled={isRecording}
          provider={transcriptModelConfig.provider}
        />
      </ModalShell>
    )}

    {modals.modelSelector && (
      <ModalShell
        title={messages.modelSelector ? 'Vælg en model til talegenkendelse' : 'Model til transskription'}
        onClose={() => onClose('modelSelector')}
        wide
        footer={
          <div className="flex w-full items-center justify-between gap-4">
            <label className="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                checked={showConfidenceIndicator}
                onChange={(e) => toggleConfidenceIndicator(e.target.checked)}
                className="h-5 w-5 accent-primary"
              />
              <span>
                <span className="block text-sm font-medium text-foreground">Markér usikker tekst</span>
                <span className="block text-xs text-muted-foreground">Viser en prik ved linjer, hvor modellen var i tvivl</span>
              </span>
            </label>
            <button
              type="button"
              onClick={() => onClose('modelSelector')}
              className="min-h-[44px] px-4 text-sm font-medium text-foreground bg-secondary rounded-md hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
            >
              {messages.modelSelector ? 'Annullér' : 'Færdig'}
            </button>
          </div>
        }>
        <TranscriptSettings
          transcriptModelConfig={transcriptModelConfig}
          setTranscriptModelConfig={setTranscriptModelConfig}
          onModelSelect={() => onClose('modelSelector')}
        />
      </ModalShell>
    )}

    {modals.errorAlert && (
      <div className="fixed inset-0 bg-foreground/50 flex items-center justify-center z-50">
        <Alert role="alertdialog" className="max-w-md mx-4 border-destructive/40 bg-card shadow-xl">
          <AlertTitle className="text-destructive">Optagelsen er stoppet</AlertTitle>
          <AlertDescription className="text-foreground">
            <p>{messages.errorAlert}</p>
            <button
              type="button"
              autoFocus
              onClick={() => onClose('errorAlert')}
              className="mt-3 min-h-[44px] px-4 rounded-md bg-secondary text-sm font-medium hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
            >
              Luk
            </button>
          </AlertDescription>
        </Alert>
      </div>
    )}

    {modals.chunkDropWarning && (
      <div className="fixed inset-0 bg-foreground/50 flex items-center justify-center z-50">
        <Alert role="alertdialog" className="max-w-lg mx-4 border-warning/40 bg-card shadow-xl">
          <AlertTitle className="text-warning">Transskriptionen kan ikke følge med</AlertTitle>
          <AlertDescription className="text-foreground">
            <p>{messages.chunkDropWarning}</p>
            <button
              type="button"
              autoFocus
              onClick={() => onClose('chunkDropWarning')}
              className="mt-3 min-h-[44px] px-4 rounded-md bg-secondary text-sm font-medium hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
            >
              Luk
            </button>
          </AlertDescription>
        </Alert>
      </div>
    )}
  </>
}

function PrimaryButton({ onClick, children }: { onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="min-h-[44px] px-5 text-sm font-semibold text-primary-foreground bg-primary rounded-md hover:bg-primary/90 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-card"
    >
      {children}
    </button>
  );
}

/** Fælles ramme til dialogerne: titel, luk-knap, Escape og fokus på dialogen. */
function ModalShell({
  title,
  onClose,
  wide = false,
  footer,
  children,
}: {
  title: string;
  onClose: () => void;
  wide?: boolean;
  footer?: React.ReactNode;
  children: React.ReactNode;
}) {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  return (
    <div className="fixed inset-0 bg-foreground/50 flex items-center justify-center z-50 p-4">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={`bg-card border border-border rounded-xl shadow-xl w-full ${wide ? 'max-w-4xl' : 'max-w-md'} max-h-[90vh] flex flex-col`}
      >
        <div className="flex justify-between items-center px-6 py-4 border-b border-border">
          <h3 className="font-serif text-xl font-semibold text-foreground">{title}</h3>
          <button
            type="button"
            onClick={onClose}
            aria-label="Luk"
            className="h-11 w-11 -mr-2 inline-flex items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
            <X className="h-5 w-5" aria-hidden="true" />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto px-6 py-5">{children}</div>
        {footer && <div className="px-6 py-4 border-t border-border flex justify-end">{footer}</div>}
      </div>
    </div>
  );
}
