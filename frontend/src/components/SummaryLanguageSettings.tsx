'use client';

import { useState } from 'react';
import { Globe, Pin } from 'lucide-react';
import { Popover, PopoverTrigger, PopoverContent } from '@/components/ui/popover';
import { LanguagePickerPopover } from '@/components/LanguagePickerPopover';
import { useRecentLanguages } from '@/hooks/useRecentLanguages';
import { labelForCode } from '@/lib/summary-languages';

export function SummaryLanguageSettings() {
  const { recents, pinned, addRecent, removeRecent, setPinned } = useRecentLanguages();
  const [pickerOpen, setPickerOpen] = useState(false);

  const togglePin = (code: string) => {
    setPinned(pinned === code ? null : code);
  };

  return (
    <div className="bg-card rounded-lg border border-border p-6 shadow-sm relative">
      <div className="flex items-center gap-2 mb-2">
        <Globe size={18} className="text-muted-foreground" />
        <h3 className="text-lg font-semibold text-foreground">Opsummeringssprog</h3>
      </div>
      <p className="text-sm text-muted-foreground mb-4">
        Fastgør ét sprog som standard for nye møder. Sprog, du ikke har fastgjort, bliver ved med at
        være hurtigvalg, når du laver en opsummering. Automatisk bruger det sprog, der fylder mest i transskriptet.
      </p>

      <div className="flex flex-wrap items-center gap-2">
        {recents.map((code) => {
          const isPinned = pinned === code;
          return (
            <span
              key={code}
              className={`inline-flex items-center rounded-full border text-sm overflow-hidden ${
                isPinned
                  ? 'bg-primary/10 border-primary/30 text-primary'
                  : 'bg-secondary border-border text-foreground'
              }`}
            >
              <button
                type="button"
                aria-label={isPinned ? `Fjern ${labelForCode(code)} som standard` : `Brug ${labelForCode(code)} som standard`}
                aria-pressed={isPinned}
                title={isPinned ? 'Klik for at fjerne som standard' : 'Klik for at bruge som standard'}
                onClick={() => togglePin(code)}
                className={`flex items-center gap-1.5 pl-3 pr-2 py-1 min-h-[24px] hover:brightness-95 active:brightness-90 focus-visible:ring-2 ring-ring rounded-l-full ${
                  isPinned ? 'text-primary' : 'text-foreground'
                }`}
              >
                <Pin
                  size={14}
                  className={isPinned ? 'text-primary' : 'text-muted-foreground'}
                  fill={isPinned ? 'currentColor' : 'none'}
                />
                {labelForCode(code)}
              </button>
              <button
                type="button"
                aria-label={`Fjern ${labelForCode(code)} fra listen`}
                onClick={() => removeRecent(code)}
                className="pr-2.5 pl-0.5 py-1 min-h-[24px] min-w-[24px] leading-none text-muted-foreground hover:text-foreground focus-visible:ring-2 ring-ring rounded-r-full"
              >
                ×
              </button>
            </span>
          );
        })}

        <Popover open={pickerOpen} onOpenChange={setPickerOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              disabled={recents.length >= 5}
              className="inline-flex items-center gap-1 rounded-full border border-dashed border-input px-3 py-1 min-h-[24px] text-sm text-muted-foreground hover:border-input hover:text-foreground focus-visible:ring-2 ring-ring disabled:cursor-not-allowed disabled:opacity-50"
            >
              ＋ Tilføj sprog
            </button>
          </PopoverTrigger>
          <PopoverContent align="start" className="w-auto p-0 border-0 shadow-none bg-transparent">
            <LanguagePickerPopover
              mode="settings"
              value={null}
              onChange={(code) => {
                if (code) addRecent(code);
                setPickerOpen(false);
              }}
              onClose={() => setPickerOpen(false)}
            />
          </PopoverContent>
        </Popover>
      </div>

      <p className="text-xs text-muted-foreground mt-3">
        {pinned
          ? `Standard: ${labelForCode(pinned)}. Klik på det igen for at fjerne standarden. Højst 5 hurtigvalg.`
          : 'Klik på et sprog for at bruge det som standard. Højst 5 hurtigvalg.'}
      </p>
    </div>
  );
}
