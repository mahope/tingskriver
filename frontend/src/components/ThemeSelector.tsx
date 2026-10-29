'use client';

import { Monitor, Moon, Sun } from 'lucide-react';
import { useTheme, ThemePreference } from '@/contexts/ThemeContext';

const OPTIONS: { value: ThemePreference; label: string; icon: typeof Sun }[] = [
  { value: 'system', label: 'Følg systemet', icon: Monitor },
  { value: 'light', label: 'Lyst', icon: Sun },
  { value: 'dark', label: 'Mørkt', icon: Moon },
];

export function ThemeSelector() {
  const { preference, setPreference } = useTheme();

  return (
    <div role="radiogroup" aria-label="Udseende" className="inline-flex rounded-lg border border-input p-0.5 bg-card">
      {OPTIONS.map(({ value, label, icon: Icon }) => {
        const active = preference === value;
        return (
          <button
            key={value}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => setPreference(value)}
            className={`inline-flex items-center gap-2 min-h-[36px] px-3 rounded-md text-sm transition-colors ${
              active ? 'bg-primary text-primary-foreground' : 'text-foreground hover:bg-accent'
            }`}
          >
            <Icon className="h-4 w-4" aria-hidden="true" />
            {label}
          </button>
        );
      })}
    </div>
  );
}
