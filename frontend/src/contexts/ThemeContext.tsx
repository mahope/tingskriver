'use client';

import { createContext, useCallback, useContext, useEffect, useState } from 'react';

export type ThemePreference = 'system' | 'light' | 'dark';

const STORAGE_KEY = 'tingskriver.theme';

interface ThemeContextValue {
  preference: ThemePreference;
  resolved: 'light' | 'dark';
  setPreference: (value: ThemePreference) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function systemPrefersDark(): boolean {
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

function applyTheme(preference: ThemePreference): 'light' | 'dark' {
  const resolved = preference === 'system' ? (systemPrefersDark() ? 'dark' : 'light') : preference;
  document.documentElement.classList.toggle('dark', resolved === 'dark');
  return resolved;
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [preference, setPreferenceState] = useState<ThemePreference>('system');
  const [resolved, setResolved] = useState<'light' | 'dark'>('light');

  useEffect(() => {
    const stored = localStorage.getItem(STORAGE_KEY);
    const initial: ThemePreference = stored === 'light' || stored === 'dark' ? stored : 'system';
    setPreferenceState(initial);
    setResolved(applyTheme(initial));
  }, []);

  useEffect(() => {
    if (preference !== 'system') return;
    const query = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => setResolved(applyTheme('system'));
    query.addEventListener('change', onChange);
    return () => query.removeEventListener('change', onChange);
  }, [preference]);

  const setPreference = useCallback((value: ThemePreference) => {
    localStorage.setItem(STORAGE_KEY, value);
    setPreferenceState(value);
    setResolved(applyTheme(value));
  }, []);

  return (
    <ThemeContext.Provider value={{ preference, resolved, setPreference }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error('useTheme skal bruges inde i ThemeProvider');
  return ctx;
}
