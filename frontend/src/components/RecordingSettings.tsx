import React, { useState, useEffect } from 'react';
import { Switch } from '@/components/ui/switch';
import { FolderOpen } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { DeviceSelection, SelectedDevices } from '@/components/DeviceSelection';
import Analytics from '@/lib/analytics';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useConfig } from '@/contexts/ConfigContext';

export interface RecordingPreferences {
  save_folder: string;
  auto_save: boolean;
  file_format: string;
  preferred_mic_device: string | null;
  preferred_system_device: string | null;
}

interface RecordingSettingsProps {
  onSave?: (preferences: RecordingPreferences) => void;
}

export function RecordingSettings({ onSave }: RecordingSettingsProps) {
  const [preferences, setPreferences] = useState<RecordingPreferences>({
    save_folder: '',
    auto_save: true,
    file_format: 'mp4',
    preferred_mic_device: null,
    preferred_system_device: null
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [showRecordingNotification, setShowRecordingNotification] = useState(true);
  const { isRecording } = useRecordingState();
  const { setSelectedDevices } = useConfig();

  // Load recording preferences on component mount
  useEffect(() => {
    const loadPreferences = async () => {
      try {
        const prefs = await invoke<RecordingPreferences>('get_recording_preferences');
        setPreferences(prefs);
      } catch (error) {
        console.error('Failed to load recording preferences:', error);
        // If loading fails, get default folder path
        try {
          const defaultPath = await invoke<string>('get_default_recordings_folder_path');
          setPreferences(prev => ({ ...prev, save_folder: defaultPath }));
        } catch (defaultError) {
          console.error('Failed to get default folder path:', defaultError);
        }
      } finally {
        setLoading(false);
      }
    };

    loadPreferences();
  }, []);

  // Load recording notification preference
  useEffect(() => {
    const loadNotificationPref = async () => {
      try {
        const { Store } = await import('@tauri-apps/plugin-store');
        const store = await Store.load('preferences.json');
        const show = await store.get<boolean>('show_recording_notification') ?? true;
        setShowRecordingNotification(show);
      } catch (error) {
        console.error('Failed to load notification preference:', error);
      }
    };
    loadNotificationPref();
  }, []);

  const handleAutoSaveToggle = async (enabled: boolean) => {
    const newPreferences = { ...preferences, auto_save: enabled };
    setPreferences(newPreferences);
    await savePreferences(newPreferences);

    // Track auto-save setting change
    await Analytics.track('auto_save_recording_toggled', {
      enabled: enabled.toString()
    });
  };

  const handleDeviceChange = async (devices: SelectedDevices) => {
    const newPreferences = {
      ...preferences,
      preferred_mic_device: devices.micDevice,
      preferred_system_device: devices.systemDevice
    };
    setPreferences(newPreferences);
    // Sync the in-memory selection ConfigContext exposes to the start path.
    // Without this, the picker only writes to disk (loaded into context once at
    // app mount), so a newly-chosen mic isn't honored until the next launch —
    // start keeps sending the stale launch-time device.
    setSelectedDevices(devices);
    await savePreferences(newPreferences);

    // Track default device preference changes
    // Note: Individual device selection analytics are tracked in DeviceSelection component
    await Analytics.track('default_devices_changed', {
      has_preferred_microphone: (!!devices.micDevice).toString(),
      has_preferred_system_audio: (!!devices.systemDevice).toString()
    });
  };

  const handleOpenFolder = async () => {
    try {
      await invoke('open_recordings_folder');
    } catch (error) {
      console.error('Failed to open recordings folder:', error);
    }
  };

  const handleNotificationToggle = async (enabled: boolean) => {
    try {
      setShowRecordingNotification(enabled);
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('preferences.json');
      await store.set('show_recording_notification', enabled);
      await store.save();
      toast.success('Indstillingen er gemt');
      await Analytics.track('recording_notification_preference_changed', {
        enabled: enabled.toString()
      });
    } catch (error) {
      console.error('Failed to save notification preference:', error);
      toast.error('Indstillingen kunne ikke gemmes', { description: 'Prøv igen. Hvis det fortsætter, så genstart Tingskriver.' });
    }
  };

  const savePreferences = async (prefs: RecordingPreferences) => {
    setSaving(true);
    try {
      await invoke('set_recording_preferences', { preferences: prefs });
      onSave?.(prefs);

      // Show success toast with device details
      const micDevice = prefs.preferred_mic_device || 'Standard';
      const systemDevice = prefs.preferred_system_device || 'Standard';
      toast.success("Enhedsvalg gemt", {
        description: `Mikrofon: ${micDevice}, systemlyd: ${systemDevice}`
      });
    } catch (error) {
      console.error('Failed to save recording preferences:', error);
      toast.error("Enhedsvalget kunne ikke gemmes", {
        description: `Prøv igen. ${error instanceof Error ? error.message : String(error)}`
      });
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="animate-pulse">
        <div className="h-4 bg-muted rounded w-1/4 mb-4"></div>
        <div className="h-8 bg-muted rounded mb-4"></div>
      </div>
    );
  }

  return (
    <div className="max-w-2xl space-y-8">
      <div>
        <h2 className="text-2xl text-foreground mb-1">Optagelser</h2>
        <p className="text-sm text-muted-foreground">
          Vælg, hvordan lyden fra dine møder gemmes.
        </p>
      </div>

      {/* Auto Save Toggle */}
      <div className="flex items-center justify-between gap-6">
        <div className="flex-1">
          <label htmlFor="rec-auto-save" className="font-medium text-foreground">Gem lydoptagelser</label>
          <div id="rec-auto-save-hint" className="text-sm text-muted-foreground">
            Gem automatisk lydfilen, når optagelsen stopper
          </div>
        </div>
        <Switch
          id="rec-auto-save"
          aria-describedby="rec-auto-save-hint"
          checked={preferences.auto_save}
          onCheckedChange={handleAutoSaveToggle}
          disabled={saving}
        />
      </div>

      {/* Folder Location - Only shown when auto_save is enabled */}
      {preferences.auto_save && (
        <div className="space-y-4">
          <div>
            <div className="font-medium text-foreground mb-1">Gemmes i</div>
            <div className="text-sm text-muted-foreground mb-3 break-all">
              {preferences.save_folder || 'Standardmappen'}
            </div>
            <button
              type="button"
              onClick={handleOpenFolder}
              className="inline-flex items-center gap-2 min-h-[44px] px-4 text-sm border border-input rounded-md hover:bg-accent transition-colors focus-visible:ring-2 ring-ring"
            >
              <FolderOpen aria-hidden="true" className="w-4 h-4" />
              Åbn mappen
            </button>
          </div>

          <p className="text-sm text-muted-foreground">
            Filerne gemmes som {preferences.file_format.toUpperCase()} og navngives efter tidspunktet, fx recording_ÅÅÅÅMMDD_TTMMSS.{preferences.file_format}
          </p>
        </div>
      )}

      {/* Info when auto_save is disabled */}
      {!preferences.auto_save && (
        <div className="p-4 border border-warning/30 rounded-lg bg-warning/10">
          <div className="text-sm text-warning">
            Lyden gemmes ikke. Slå "Gem lydoptagelser" til, hvis du vil beholde lyden fra dine møder.
          </div>
        </div>
      )}

      {/* Recording Notification Toggle */}
      <div className="flex items-center justify-between gap-6">
        <div className="flex-1">
          <label htmlFor="rec-start-notice" className="font-medium text-foreground">Påmindelse ved optagelsens start</label>
          <div id="rec-start-notice-hint" className="text-sm text-muted-foreground">
            Vis en påmindelse om at fortælle deltagerne, at mødet optages
          </div>
        </div>
        <Switch
          id="rec-start-notice"
          aria-describedby="rec-start-notice-hint"
          checked={showRecordingNotification}
          onCheckedChange={handleNotificationToggle}
        />
      </div>

      {/* Device Preferences */}
      <div className="space-y-4">
        <div className="border-t border-border pt-8">
          <h3 className="text-xl text-foreground mb-1">Standardenheder til lyd</h3>
          <p className="text-sm text-muted-foreground mb-4">
            Vælg den mikrofon og systemlyd, du helst vil optage med. De vælges automatisk, når du starter en ny optagelse.
          </p>

          {isRecording && (
            <p
              role="status"
              aria-live="polite"
              className="text-sm text-warning bg-warning/10 border border-warning/30 rounded-md p-2 mb-4"
            >
              Enhedsvalget er låst, mens en optagelse kører. Tilslutter du en ny enhed undervejs, skifter Tingskriver ikke til den. Stop optagelsen for at ændre enhed.
            </p>
          )}

          <div>
            <DeviceSelection
              selectedDevices={{
                micDevice: preferences.preferred_mic_device,
                systemDevice: preferences.preferred_system_device
              }}
              onDeviceChange={handleDeviceChange}
              disabled={saving || isRecording}
            />
          </div>
        </div>
      </div>
    </div>
  );
}