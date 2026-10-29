import React, { useEffect } from 'react';
import { useOnboarding } from '@/contexts/OnboardingContext';
import {
  WelcomeStep,
  PermissionsStep,
  DownloadProgressStep,
} from './steps';

interface OnboardingFlowProps {
  onComplete: () => void;
}

export function OnboardingFlow({ onComplete }: OnboardingFlowProps) {
  const { currentStep, goToStep } = useOnboarding();
  const [isMac, setIsMac] = React.useState(false);

  useEffect(() => {
    // Check if running on macOS
    const checkPlatform = async () => {
      try {
        // Dynamic import to avoid SSR issues if any
        const { platform } = await import('@tauri-apps/plugin-os');
        setIsMac(platform() === 'macos');
      } catch (e) {
        console.error('Failed to detect platform:', e);
        // Fallback
        setIsMac(navigator.userAgent.includes('Mac'));
      }
    };
    checkPlatform();
  }, []);

  // Internal step numbers are unchanged (saved status uses them):
  //   1 Welcome, 2 Setup overview (SetupOverviewStep, skipped), 3 Model download, 4 Permissions (macOS only).
  // Step 2 is skipped by jumping straight to step 3; the database is initialised in OnboardingContext.
  useEffect(() => {
    if (currentStep === 2) goToStep(3);
  }, [currentStep, goToStep]);

  // Visible numbering: Welcome = 1, Download = 2, Permissions = 3 (macOS only).
  const totalSteps = isMac ? 3 : 2;

  return (
    <div className="onboarding-flow">
      {currentStep === 1 && <WelcomeStep totalSteps={totalSteps} />}
      {currentStep === 3 && <DownloadProgressStep totalSteps={totalSteps} />}
      {currentStep === 4 && isMac && <PermissionsStep totalSteps={totalSteps} />}
    </div>
  );
}
