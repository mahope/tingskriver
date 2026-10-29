import React, { useContext, useState, useEffect } from 'react';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import { Info, Loader2, Copy, Check } from 'lucide-react';
import { AnalyticsContext } from './AnalyticsProvider';
import { load } from '@tauri-apps/plugin-store';
import { invoke } from '@tauri-apps/api/core';
import { Analytics } from '@/lib/analytics';
import AnalyticsDataModal from './AnalyticsDataModal';

const ANALYTICS_DEFAULT_OFF_MIGRATION_KEY = 'analyticsDefaultOffMigrationV1';

export default function AnalyticsConsentSwitch() {
  const { setIsAnalyticsOptedIn, isAnalyticsOptedIn } = useContext(AnalyticsContext);
  const [isProcessing, setIsProcessing] = useState(false);
  const [showModal, setShowModal] = useState(false);
  const [userId, setUserId] = useState<string>('');
  const [isCopied, setIsCopied] = useState(false);

  // Note: Store loading is handled by AnalyticsProvider to avoid race conditions

  useEffect(() => {
    const loadUserId = async () => {
      if (isAnalyticsOptedIn) {
        try {
          const id = await Analytics.getPersistentUserId();
          setUserId(id);
        } catch (error) {
          console.error('Failed to load user ID:', error);
        }
      } else {
        setUserId('');
      }
    };
    loadUserId();
  }, [isAnalyticsOptedIn]);

  const handleCopyUserId = async () => {
    if (!userId) return;

    try {
      await navigator.clipboard.writeText(userId);
      setIsCopied(true);
      setTimeout(() => setIsCopied(false), 2000);

      // Track that user copied their ID
      await Analytics.track('user_id_copied', {
        user_id: userId
      });
    } catch (error) {
      console.error('Failed to copy user ID:', error);
    }
  };

  const handleToggle = async (enabled: boolean) => {
    // If user is trying to DISABLE, show the modal first
    if (!enabled) {
      setShowModal(true);
      // Track that user viewed the transparency modal
      try {
        await invoke('track_analytics_transparency_viewed');
      } catch (error) {
        console.error('Failed to track transparency view:', error);
      }
      return; // Don't disable yet, wait for modal confirmation
    }

    // If ENABLING, proceed immediately
    await performToggle(enabled);
  };

  const performToggle = async (enabled: boolean) => {
    // Optimistic update - immediately update UI state
    setIsAnalyticsOptedIn(enabled);
    setIsProcessing(true);

    try {
      const store = await load('analytics.json', {
        autoSave: false,
        defaults: {
          analyticsOptedIn: false
        }
      });
      await store.set('analyticsOptedIn', enabled);
      await store.set(ANALYTICS_DEFAULT_OFF_MIGRATION_KEY, true);
      await store.save();

      if (enabled) {
        // Full analytics initialization (same as AnalyticsProvider)
        const userId = await Analytics.getPersistentUserId();

        // Initialize analytics
        await Analytics.init();

        // Identify user with enhanced properties immediately after init
        await Analytics.identify(userId, {
          app_version: '0.4.1',
          platform: 'tauri',
          first_seen: new Date().toISOString(),
          os: navigator.platform,
        });

        // Start analytics session with the same user ID
        await Analytics.startSession(userId);

        // Track app started (re-enabled)
        await Analytics.trackAppStarted();

        // Track that user enabled analytics
        try {
          await invoke('track_analytics_enabled');
        } catch (error) {
          console.error('Failed to track analytics enabled:', error);
        }

        console.log('Analytics re-enabled successfully');
      } else {
        // Track that user disabled analytics BEFORE disabling
        try {
          await invoke('track_analytics_disabled');
        } catch (error) {
          console.error('Failed to track analytics disabled:', error);
        }

        await Analytics.disable();
        console.log('Analytics disabled successfully');
      }
    } catch (error) {
      console.error('Failed to toggle analytics:', error);
      // Revert the optimistic update on error
      setIsAnalyticsOptedIn(!enabled);
      // You could also show a toast notification here to inform the user
    } finally {
      setIsProcessing(false);
    }
  };

  const handleConfirmDisable = async () => {
    setShowModal(false);
    await performToggle(false);
  };

  const handleCancelDisable = () => {
    setShowModal(false);
    // Keep analytics enabled, no state change needed
  };

  const handlePrivacyPolicyClick = async () => {
    try {
      await invoke('open_external_url', { url: 'https://github.com/mahope/tingskriver/blob/main/PRIVACY_POLICY.md' });
    } catch (error) {
      console.error('Failed to open privacy policy link:', error);
    }
  };

  return (
    <>
      <div className="space-y-4">
        <div>
          <h3 className="text-base font-semibold text-foreground mb-2">Anonym brugsstatistik</h3>
          <p className="text-sm text-muted-foreground mb-4">
            Brugsstatistik er slået fra som standard. Du kan slå den til for at dele anonyme data om brug og ydeevne. Der indsamles aldrig personligt indhold.
          </p>
        </div>

        <div className="flex items-center justify-between p-3 bg-background rounded-lg border border-border">
          <div>
            <h4 className="font-semibold text-foreground">Del anonym brugsstatistik</h4>
            <p className="text-sm text-muted-foreground">
              {isProcessing ? 'Opdaterer…' : 'Slået fra, medmindre du vælger at slå den til'}
            </p>
          </div>
          <div className="flex items-center gap-2 ml-4">
            {isProcessing && (
              <Loader2 aria-hidden="true" className="w-4 h-4 animate-spin text-muted-foreground" />
            )}
            <Switch
              aria-label="Del anonym brugsstatistik"
              checked={isAnalyticsOptedIn}
              onCheckedChange={handleToggle}
              disabled={isProcessing}
            />
          </div>
        </div>

        {/* User ID Display */}
        {isAnalyticsOptedIn && userId && (
          <div className="p-4 border rounded-lg bg-background">
            <div className="flex items-start justify-between gap-4">
              <div className="flex-1 min-w-0">
                <div className="font-medium text-foreground mb-1">Dit bruger-id</div>
                <p className="text-xs text-muted-foreground mb-2">
                  Oplys dette id, når du melder en fejl, så kan vi finde de relevante logs.
                </p>
                <div className="flex items-center gap-2">
                  <code className="text-xs text-foreground bg-card px-2 py-1 rounded border border-input font-mono flex-1 truncate">
                    {userId}
                  </code>
                  <Button
                    onClick={handleCopyUserId}
                    variant="outline"
                    size="sm"
                    className="flex-shrink-0"
                    title="Kopiér bruger-id"
                    aria-label="Kopiér bruger-id"
                  >
                    {isCopied ? (
                      <>
                        <Check aria-hidden="true" className="w-3.5 h-3.5 text-success" />
                        <span className="text-success">Kopieret</span>
                      </>
                    ) : (
                      <>
                        <Copy aria-hidden="true" className="w-3.5 h-3.5" />
                        <span>Kopiér</span>
                      </>
                    )}
                  </Button>
                </div>
              </div>
            </div>
          </div>
        )}

        <div className="flex items-start gap-2 p-2 bg-primary/10 rounded border border-primary/30">
          <Info aria-hidden="true" className="w-4 h-4 text-primary mt-0.5 flex-shrink-0" />
          <div className="text-sm text-primary">
            <p className="mb-1">
              Dine møder, transskripter og optagelser forbliver private og ligger kun på din computer.
            </p>
            <button
              type="button"
              onClick={handlePrivacyPolicyClick}
              className="text-primary underline hover:no-underline focus-visible:ring-2 ring-ring rounded-sm"
            >
              Læs privatlivspolitikken
            </button>
          </div>
        </div>
      </div>

      {/* 2-Step Opt-Out Modal */}
      <AnalyticsDataModal
        isOpen={showModal}
        onClose={handleCancelDisable}
        onConfirmDisable={handleConfirmDisable}
      />
    </>
  );
}
