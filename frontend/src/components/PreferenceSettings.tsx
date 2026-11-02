"use client"

import { useEffect, useState, useRef } from "react"
import { Switch } from "./ui/switch"
import { FolderOpen } from "lucide-react"
import { invoke } from "@tauri-apps/api/core"
import Analytics from "@/lib/analytics"
import { ThemeSelector } from "@/components/ThemeSelector"
import { useConfig, NotificationSettings } from "@/contexts/ConfigContext"
import { usePlatform } from "@/hooks/usePlatform"

export function PreferenceSettings() {
  const {
    notificationSettings,
    storageLocations,
    isLoadingPreferences,
    loadPreferences,
    updateNotificationSettings
  } = useConfig();

  const [notificationsEnabled, setNotificationsEnabled] = useState<boolean | null>(null);
  const [isInitialLoad, setIsInitialLoad] = useState(true);
  const [previousNotificationsEnabled, setPreviousNotificationsEnabled] = useState<boolean | null>(null);
  const [crashDumpsEnabled, setCrashDumpsEnabled] = useState<boolean | null>(null);
  const [crashDumpsError, setCrashDumpsError] = useState<string | null>(null);
  // Crash dumps are written by Windows Error Reporting only.
  const isWindows = usePlatform() === 'windows';

  useEffect(() => {
    invoke<boolean>('get_crash_dumps_enabled')
      .then(setCrashDumpsEnabled)
      .catch((error) => console.error('Failed to read crash dump setting:', error));
  }, []);

  const handleCrashDumpsChange = async (enabled: boolean) => {
    const previous = crashDumpsEnabled;
    setCrashDumpsEnabled(enabled);
    setCrashDumpsError(null);
    try {
      await invoke('set_crash_dumps_enabled', { enabled });
    } catch (error) {
      console.error('Failed to change crash dump setting:', error);
      setCrashDumpsEnabled(previous);
      setCrashDumpsError('Indstillingen kunne ikke gemmes. Prøv igen.');
    }
  };
  const hasTrackedViewRef = useRef(false);

  // Lazy load preferences on mount (only loads if not already cached)
  useEffect(() => {
    loadPreferences();
    // Reset tracking ref on mount (every tab visit)
    hasTrackedViewRef.current = false;
  }, [loadPreferences]);

  // Track preferences viewed analytics on every tab visit (once per mount)
  useEffect(() => {
    if (hasTrackedViewRef.current) return;

    const trackPreferencesViewed = async () => {
      // Wait for notification settings to be available (either from cache or after loading)
      if (notificationSettings) {
        await Analytics.track('preferences_viewed', {
          notifications_enabled: notificationSettings.notification_preferences.show_recording_started ? 'true' : 'false'
        });
        hasTrackedViewRef.current = true;
      } else if (!isLoadingPreferences) {
        // If not loading and no settings available, track with default value
        await Analytics.track('preferences_viewed', {
          notifications_enabled: 'false'
        });
        hasTrackedViewRef.current = true;
      }
    };

    trackPreferencesViewed();
  }, [notificationSettings, isLoadingPreferences]);

  // Update notificationsEnabled when notificationSettings are loaded from global state
  useEffect(() => {
    if (notificationSettings) {
      // Notification enabled means both started and stopped notifications are enabled
      const enabled =
        notificationSettings.notification_preferences.show_recording_started &&
        notificationSettings.notification_preferences.show_recording_stopped;
      setNotificationsEnabled(enabled);
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(enabled);
        setIsInitialLoad(false);
      }
    } else if (!isLoadingPreferences) {
      // If not loading and no settings, use default
      setNotificationsEnabled(true);
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(true);
        setIsInitialLoad(false);
      }
    }
  }, [notificationSettings, isLoadingPreferences, isInitialLoad])

  useEffect(() => {
    // Skip update on initial load or if value hasn't actually changed
    if (isInitialLoad || notificationsEnabled === null || notificationsEnabled === previousNotificationsEnabled) return;
    if (!notificationSettings) return;

    const handleUpdateNotificationSettings = async () => {
      console.log("Updating notification settings to:", notificationsEnabled);

      try {
        // Update the notification preferences
        const updatedSettings: NotificationSettings = {
          ...notificationSettings,
          notification_preferences: {
            ...notificationSettings.notification_preferences,
            show_recording_started: notificationsEnabled,
            show_recording_stopped: notificationsEnabled,
          }
        };

        console.log("Calling updateNotificationSettings with:", updatedSettings);
        await updateNotificationSettings(updatedSettings);
        setPreviousNotificationsEnabled(notificationsEnabled);
        console.log("Successfully updated notification settings to:", notificationsEnabled);

        // Track notification preference change - only fires when user manually toggles
        await Analytics.track('notification_settings_changed', {
          notifications_enabled: notificationsEnabled.toString()
        });
      } catch (error) {
        console.error('Failed to update notification settings:', error);
      }
    };

    handleUpdateNotificationSettings();
  }, [notificationsEnabled, notificationSettings, isInitialLoad, previousNotificationsEnabled, updateNotificationSettings])

  const handleOpenFolder = async (folderType: 'database' | 'models' | 'recordings' | 'logs') => {
    try {
      switch (folderType) {
        case 'database':
          await invoke('open_database_folder');
          break;
        case 'models':
          await invoke('open_models_folder');
          break;
        case 'recordings':
          await invoke('open_recordings_folder');
          break;
        case 'logs':
          await invoke('open_log_folder');
          break;
      }

      // Track storage folder access
      await Analytics.track('storage_folder_opened', {
        folder_type: folderType
      });
    } catch (error) {
      console.error(`Failed to open ${folderType} folder:`, error);
    }
  };

  // Show loading only if we're actually loading and don't have cached data
  if (isLoadingPreferences && !notificationSettings && !storageLocations) {
    return <div className="max-w-2xl mx-auto p-6 text-muted-foreground" role="status">Henter indstillinger…</div>
  }

  // Show loading if notificationsEnabled hasn't been determined yet
  if (notificationsEnabled === null && !isLoadingPreferences) {
    return <div className="max-w-2xl mx-auto p-6 text-muted-foreground" role="status">Henter indstillinger…</div>
  }

  // Ensure we have a boolean value for the Switch component
  const notificationsEnabledValue = notificationsEnabled ?? false;

  return (
    <div className="max-w-2xl space-y-10">
      {/* Appearance */}
      <section aria-labelledby="pref-appearance">
        <h2 id="pref-appearance" className="text-2xl text-foreground mb-1">Udseende</h2>
        <p className="text-sm text-muted-foreground mb-4">
          Vælg et lyst eller mørkt udseende, eller lad Tingskriver følge din computers indstilling.
        </p>
        <ThemeSelector />
      </section>

      {/* Notifications */}
      <section aria-labelledby="pref-notifications" className="border-t border-border pt-8">
        <h2 id="pref-notifications" className="text-2xl text-foreground mb-1">Notifikationer</h2>
        <p className="text-sm text-muted-foreground mb-4">
          Få en besked, når en optagelse starter og stopper.
        </p>
        <div className="flex items-center justify-between gap-6">
          <div>
            <label htmlFor="pref-notifications-switch" className="font-medium text-foreground">
              Notifikationer om start og slut
            </label>
            <p id="pref-notifications-hint" className="text-sm text-muted-foreground">
              Slå fra, hvis du ikke vil forstyrres under møder.
            </p>
          </div>
          <Switch
            id="pref-notifications-switch"
            aria-describedby="pref-notifications-hint"
            checked={notificationsEnabledValue}
            onCheckedChange={setNotificationsEnabled}
          />
        </div>
      </section>

      {/* Data storage */}
      <section aria-labelledby="pref-storage" className="border-t border-border pt-8">
        <h2 id="pref-storage" className="text-2xl text-foreground mb-1">Placering af data</h2>
        <p className="text-sm text-muted-foreground mb-4">
          Her gemmer Tingskriver dine optagelser på din computer.
        </p>

        <div className="font-medium text-foreground mb-1">Mødeoptagelser</div>
        {storageLocations?.recordings ? (
          <div className="text-sm text-muted-foreground mb-3 break-all font-mono">
            {storageLocations.recordings}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground mb-3">
            Placeringen kunne ikke hentes. Genstart Tingskriver, og prøv igen.
          </p>
        )}
        <button
          type="button"
          onClick={() => handleOpenFolder('recordings')}
          disabled={!storageLocations?.recordings}
          className="inline-flex items-center gap-2 min-h-[44px] px-4 text-sm border border-input rounded-md hover:bg-accent transition-colors focus-visible:ring-2 ring-ring disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <FolderOpen className="w-4 h-4" aria-hidden="true" />
          Åbn mappen
        </button>

        <div className="font-medium text-foreground mt-6 mb-1">Logfiler</div>
        <p className="text-sm text-muted-foreground mb-3">
          Appens log, fx skift af lydenhed under en optagelse. Send mappen, hvis noget fejler.
        </p>
        <button
          type="button"
          onClick={() => handleOpenFolder('logs')}
          className="inline-flex items-center gap-2 min-h-[44px] px-4 text-sm border border-input rounded-md hover:bg-accent transition-colors focus-visible:ring-2 ring-ring"
        >
          <FolderOpen className="w-4 h-4" aria-hidden="true" />
          Åbn logmappe
        </button>

        {isWindows && (<>
        <div className="flex items-center justify-between gap-6 mt-6">
          <div>
            <label htmlFor="pref-crash-dumps-switch" className="font-medium text-foreground">
              Gem nedbrudsfiler
            </label>
            <p id="pref-crash-dumps-hint" className="text-sm text-muted-foreground">
              Går Tingskriver ned, gemmer Windows en lille fil i mappen dumps ved siden af loggen.
              Den bliver på din computer, indtil du selv sender den.
            </p>
          </div>
          <Switch
            id="pref-crash-dumps-switch"
            aria-describedby="pref-crash-dumps-hint"
            checked={crashDumpsEnabled ?? true}
            disabled={crashDumpsEnabled === null}
            onCheckedChange={handleCrashDumpsChange}
          />
        </div>
        {crashDumpsError && (
          <p className="mt-2 text-sm text-destructive" role="alert">{crashDumpsError}</p>
        )}
        </>)}

        <p className="mt-6 text-sm text-muted-foreground">
          Databasen og modellerne ligger sammen i programmets datamappe, så alt er samlet ét sted.
        </p>
      </section>
    </div>
  )
}
