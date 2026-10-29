import React, { useState, useEffect } from 'react';
import { Globe } from 'lucide-react';
import Analytics from '@/lib/analytics';
import { toast } from 'sonner';
import { useConfig } from '@/contexts/ConfigContext';

export interface Language {
  code: string;
  name: string;
}

// ISO 639-1 language codes supported by Whisper
const LANGUAGES: Language[] = [
  { code: 'auto', name: 'Automatisk (originalsproget)' },
  { code: 'auto-translate', name: 'Automatisk (oversæt til engelsk)' },
  { code: 'en', name: 'Engelsk' },
  { code: 'zh', name: 'Kinesisk' },
  { code: 'de', name: 'Tysk' },
  { code: 'es', name: 'Spansk' },
  { code: 'ru', name: 'Russisk' },
  { code: 'ko', name: 'Koreansk' },
  { code: 'fr', name: 'Fransk' },
  { code: 'ja', name: 'Japansk' },
  { code: 'pt', name: 'Portugisisk' },
  { code: 'tr', name: 'Tyrkisk' },
  { code: 'pl', name: 'Polsk' },
  { code: 'ca', name: 'Catalansk' },
  { code: 'nl', name: 'Hollandsk' },
  { code: 'ar', name: 'Arabisk' },
  { code: 'sv', name: 'Svensk' },
  { code: 'it', name: 'Italiensk' },
  { code: 'id', name: 'Indonesisk' },
  { code: 'hi', name: 'Hindi' },
  { code: 'fi', name: 'Finsk' },
  { code: 'vi', name: 'Vietnamesisk' },
  { code: 'he', name: 'Hebraisk' },
  { code: 'uk', name: 'Ukrainsk' },
  { code: 'el', name: 'Græsk' },
  { code: 'ms', name: 'Malajisk' },
  { code: 'cs', name: 'Tjekkisk' },
  { code: 'ro', name: 'Rumænsk' },
  { code: 'da', name: 'Dansk' },
  { code: 'hu', name: 'Ungarsk' },
  { code: 'ta', name: 'Tamil' },
  { code: 'no', name: 'Norsk' },
  { code: 'th', name: 'Thai' },
  { code: 'ur', name: 'Urdu' },
  { code: 'hr', name: 'Kroatisk' },
  { code: 'bg', name: 'Bulgarsk' },
  { code: 'lt', name: 'Litauisk' },
  { code: 'la', name: 'Latin' },
  { code: 'mi', name: 'Maori' },
  { code: 'ml', name: 'Malayalam' },
  { code: 'cy', name: 'Walisisk' },
  { code: 'sk', name: 'Slovakisk' },
  { code: 'te', name: 'Telugu' },
  { code: 'fa', name: 'Persisk' },
  { code: 'lv', name: 'Lettisk' },
  { code: 'bn', name: 'Bengali' },
  { code: 'sr', name: 'Serbisk' },
  { code: 'az', name: 'Aserbajdsjansk' },
  { code: 'sl', name: 'Slovensk' },
  { code: 'kn', name: 'Kannada' },
  { code: 'et', name: 'Estisk' },
  { code: 'mk', name: 'Makedonsk' },
  { code: 'br', name: 'Bretonsk' },
  { code: 'eu', name: 'Baskisk' },
  { code: 'is', name: 'Islandsk' },
  { code: 'hy', name: 'Armensk' },
  { code: 'ne', name: 'Nepalesisk' },
  { code: 'mn', name: 'Mongolsk' },
  { code: 'bs', name: 'Bosnisk' },
  { code: 'kk', name: 'Kasakhisk' },
  { code: 'sq', name: 'Albansk' },
  { code: 'sw', name: 'Swahili' },
  { code: 'gl', name: 'Galicisk' },
  { code: 'mr', name: 'Marathi' },
  { code: 'pa', name: 'Punjabi' },
  { code: 'si', name: 'Singalesisk' },
  { code: 'km', name: 'Khmer' },
  { code: 'sn', name: 'Shona' },
  { code: 'yo', name: 'Yoruba' },
  { code: 'so', name: 'Somali' },
  { code: 'af', name: 'Afrikaans' },
  { code: 'oc', name: 'Occitansk' },
  { code: 'ka', name: 'Georgisk' },
  { code: 'be', name: 'Hviderussisk' },
  { code: 'tg', name: 'Tadsjikisk' },
  { code: 'sd', name: 'Sindhi' },
  { code: 'gu', name: 'Gujarati' },
  { code: 'am', name: 'Amharisk' },
  { code: 'yi', name: 'Jiddisch' },
  { code: 'lo', name: 'Laotisk' },
  { code: 'uz', name: 'Usbekisk' },
  { code: 'fo', name: 'Færøsk' },
  { code: 'ht', name: 'Haitiansk kreolsk' },
  { code: 'ps', name: 'Pashto' },
  { code: 'tk', name: 'Turkmensk' },
  { code: 'nn', name: 'Nynorsk' },
  { code: 'mt', name: 'Maltesisk' },
  { code: 'sa', name: 'Sanskrit' },
  { code: 'lb', name: 'Luxembourgsk' },
  { code: 'my', name: 'Burmesisk' },
  { code: 'bo', name: 'Tibetansk' },
  { code: 'tl', name: 'Tagalog' },
  { code: 'mg', name: 'Malagassisk' },
  { code: 'as', name: 'Assamesisk' },
  { code: 'tt', name: 'Tatarisk' },
  { code: 'haw', name: 'Hawaiiansk' },
  { code: 'ln', name: 'Lingala' },
  { code: 'ha', name: 'Hausa' },
  { code: 'ba', name: 'Basjkirsk' },
  { code: 'jw', name: 'Javanesisk' },
  { code: 'su', name: 'Sundanesisk' },
];

interface LanguageSelectionProps {
  selectedLanguage: string;
  onLanguageChange: (language: string) => void;
  disabled?: boolean;
  provider?: 'localWhisper' | 'parakeet' | 'deepgram' | 'elevenLabs' | 'groq' | 'openai';
}

export function LanguageSelection({
  selectedLanguage,
  onLanguageChange,
  disabled = false,
  provider = 'localWhisper'
}: LanguageSelectionProps) {
  const [saving, setSaving] = useState(false);
  const { setSelectedLanguage } = useConfig();

  // Parakeet only supports auto-detection (doesn't support manual language selection)
  const isParakeet = provider === 'parakeet';
  const availableLanguages = isParakeet
    ? LANGUAGES.filter(lang => lang.code === 'auto' || lang.code === 'auto-translate')
    : LANGUAGES;

  const handleLanguageChange = async (languageCode: string) => {
    setSaving(true);
    try {
      // Save language preference to localStorage and sync to backend
      setSelectedLanguage(languageCode);
      onLanguageChange(languageCode);
      console.log('Language preference saved:', languageCode);

      // Track language selection analytics
      const selectedLang = LANGUAGES.find(lang => lang.code === languageCode);
      await Analytics.track('language_selected', {
        language_code: languageCode,
        language_name: selectedLang?.code || 'unknown',
        is_auto_detect: (languageCode === 'auto').toString(),
        is_auto_translate: (languageCode === 'auto-translate').toString()
      });

      // Show success toast
      const languageName = selectedLang?.name || languageCode;
      toast.success("Sprog gemt", {
        description: `Transskriptionssproget er sat til ${languageName}`
      });
    } catch (error) {
      console.error('Failed to save language preference:', error);
      toast.error("Sproget kunne ikke gemmes", {
        description: `Prøv igen. ${error instanceof Error ? error.message : String(error)}`
      });
    } finally {
      setSaving(false);
    }
  };

  // Find the selected language name for display
  const selectedLanguageName = LANGUAGES.find(
    lang => lang.code === selectedLanguage
  )?.name || 'Automatisk (originalsproget)';

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Globe aria-hidden="true" className="h-4 w-4 text-muted-foreground" />
          <label htmlFor="transcription-language" className="text-sm font-medium text-foreground">Transskriptionssprog</label>
        </div>
      </div>

      <div className="space-y-2">
        <select
          id="transcription-language"
          aria-describedby="transcription-language-hint"
          value={selectedLanguage}
          onChange={(e) => handleLanguageChange(e.target.value)}
          disabled={disabled || saving}
          className="w-full px-3 py-2 text-sm bg-card border border-input rounded-md shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus:border-ring disabled:bg-background disabled:text-muted-foreground"
        >
          {availableLanguages.map((language) => (
            <option key={language.code} value={language.code}>
              {language.name}
              {language.code !== 'auto' && language.code !== 'auto-translate' && ` (${language.code})`}
            </option>
          ))}
        </select>

        <p id="transcription-language-hint" className="text-xs text-muted-foreground">Vælg det sprog, der tales i dine møder.</p>

        {/* Parakeet language limitation warning */}
        {isParakeet && (
          <div className="p-2 bg-warning/10 border border-warning/30 rounded text-warning">
            <p className="font-medium">Sprogvalg med Parakeet</p>
            <p className="mt-1 text-xs">Parakeet kan kun genkende sproget automatisk. Skift til Whisper, hvis du selv vil vælge sprog.</p>
          </div>
        )}

        {/* Info text */}
        <div className="text-xs space-y-2 pt-2">
          <p className="text-muted-foreground">
            <strong>Valgt:</strong> {selectedLanguageName}
          </p>
          {selectedLanguage === 'auto' && (
            <div className="p-2 bg-warning/10 border border-warning/30 rounded text-warning">
              <p className="font-medium">Automatisk genkendelse kan give fejl</p>
              <p className="mt-1">Vælg dit sprog (fx dansk eller engelsk) for at få det bedste resultat.</p>
            </div>
          )}
          {selectedLanguage === 'auto-translate' && (
            <div className="p-2 bg-primary/10 border border-primary/30 rounded text-primary">
              <p className="font-medium">Oversættelse til engelsk er slået til</p>
              <p className="mt-1">Al tale oversættes automatisk til engelsk. Velegnet til møder på flere sprog, hvor du har brug for engelsk tekst.</p>
            </div>
          )}
          {selectedLanguage !== 'auto' && selectedLanguage !== 'auto-translate' && (
            <p className="text-muted-foreground">
              Transskriptionen tilpasses <strong>{selectedLanguageName}</strong>
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
