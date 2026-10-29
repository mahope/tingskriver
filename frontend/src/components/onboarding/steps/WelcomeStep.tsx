import React from 'react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';

export function WelcomeStep({ totalSteps = 2 }: { totalSteps?: number }) {
  const { goNext } = useOnboarding();

  return (
    <OnboardingContainer
      title="Velkommen til Tingskriver"
      description="Optag, transskribér og opsummér dine møder. Alt kører lokalt på din computer, og dine optagelser forlader aldrig den."
      step={1}
      totalSteps={totalSteps}
    >
      <div className="flex flex-col items-center space-y-4">
        <p className="max-w-md text-center text-sm text-muted-foreground">
          Først skal du hente en transskriptionsmodel. Det tager få minutter.
        </p>
        <div className="w-full max-w-xs">
          <Button onClick={goNext} className="w-full min-h-[44px] focus-visible:ring-2">
            Kom i gang
          </Button>
        </div>
      </div>
    </OnboardingContainer>
  );
}
