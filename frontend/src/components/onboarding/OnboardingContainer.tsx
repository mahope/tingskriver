import React from 'react';
import { cn } from '@/lib/utils';
import { ProgressIndicator } from './shared/ProgressIndicator';
import type { OnboardingContainerProps } from '@/types/onboarding';

export function OnboardingContainer({
  title,
  description,
  children,
  step,
  totalSteps = 3,
  hideProgress = false,
  className,
}: OnboardingContainerProps) {
  return (
    <div className="fixed inset-0 bg-background flex items-center justify-center z-50 overflow-hidden">
      <div className={cn('w-full max-w-2xl h-full max-h-screen flex flex-col px-6 py-8', className)}>
        {step && !hideProgress && (
          <div className="flex-shrink-0">
            <ProgressIndicator current={step} total={totalSteps} />
          </div>
        )}

        <div className="mb-6 text-center space-y-3 flex-shrink-0">
          <h1 className="text-3xl font-semibold text-foreground">{title}</h1>
          {description && (
            <p className="text-base text-muted-foreground max-w-md mx-auto">{description}</p>
          )}
        </div>

        <div className="flex-1 overflow-y-auto pr-2">
          <div className="space-y-6">{children}</div>
        </div>
      </div>
    </div>
  );
}
