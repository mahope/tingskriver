'use client';

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog';
import { Loader2, FolderOpen, Database, CheckCircle2, XCircle } from 'lucide-react';
import { HomebrewDatabaseDetector } from './HomebrewDatabaseDetector';

interface LegacyDatabaseImportProps {
  isOpen: boolean;
  onComplete: () => void;
}

type ImportState = 'idle' | 'selecting' | 'detecting' | 'importing' | 'success' | 'error';

export function LegacyDatabaseImport({ isOpen, onComplete }: LegacyDatabaseImportProps) {
  const [importState, setImportState] = useState<ImportState>('idle');
  const [detectedPath, setDetectedPath] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string>('');

  const handleBrowse = async () => {
    try {
      setImportState('selecting');

      // Open file picker
      const selectedPath = await invoke<string | null>('select_legacy_database_path');

      if (!selectedPath) {
        setImportState('idle');
        return;
      }

      setImportState('detecting');

      // Detect database from selected path
      const dbPath = await invoke<string | null>('detect_legacy_database', {
        selectedPath,
      });

      if (dbPath) {
        setDetectedPath(dbPath);
        setImportState('idle');
      } else {
        setErrorMessage('Der er ingen database det sted. Vælg mappen fra den tidligere installation, backend-mappen eller selve databasefilen.');
        setDetectedPath(null);
        setImportState('error');
        setTimeout(() => setImportState('idle'), 3000);
      }
    } catch (error) {
      console.error('Error browsing for database:', error);
      setErrorMessage('Databasen kunne ikke åbnes. Prøv at vælge filen igen.');
      setImportState('error');
      setTimeout(() => setImportState('idle'), 3000);
    }
  };

  const handleImport = async () => {
    if (!detectedPath) return;

    try {
      setImportState('importing');

      await invoke('import_and_initialize_database', {
        legacyDbPath: detectedPath,
      });

      setImportState('success');
      toast.success('Databasen er importeret. Genindlæser…');

      // Wait 1 second for user to see success, then reload window to refresh all data
      setTimeout(() => {
        window.location.reload();
      }, 1000);
    } catch (error) {
      console.error('Error importing database:', error);
      setErrorMessage('Importen mislykkedes. Tjek at filen er en gyldig database, og prøv igen.');
      setImportState('error');
      toast.error('Importen mislykkedes. Prøv igen.');
      setTimeout(() => setImportState('idle'), 3000);
    }
  };

  const handleStartFresh = async () => {
    try {
      setImportState('importing');

      await invoke('initialize_fresh_database');

      setImportState('success');
      toast.success('Databasen er klar. Starter appen…');

      // Wait 1 second for user to see success, then reload window to start fresh
      setTimeout(() => {
        window.location.reload();
      }, 1000);
    } catch (error) {
      console.error('Error initializing database:', error);
      setErrorMessage('Databasen kunne ikke oprettes. Genstart appen og prøv igen.');
      setImportState('error');
      toast.error('Databasen kunne ikke oprettes. Genstart appen og prøv igen.');
      setTimeout(() => setImportState('idle'), 3000);
    }
  };

  const isLoading = ['selecting', 'detecting', 'importing'].includes(importState);
  const canImport = detectedPath && importState === 'idle';

  const handleHomebrewImportSuccess = () => {
    // The HomebrewDatabaseDetector handles the reload itself
    onComplete();
  };

  const handleHomebrewDecline = () => {
    // User declined homebrew import, they can continue with manual browse
  };

  return (
    <Dialog open={isOpen} onOpenChange={() => {}}>
      <DialogContent className="sm:max-w-[600px]" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle className="text-2xl">Velkommen til Tingskriver</DialogTitle>
          <DialogDescription className="text-base pt-2">
            Har du data fra en tidligere installation?
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-6 py-4">
          {/* Homebrew Database Auto-Detection */}
          <HomebrewDatabaseDetector 
            onImportSuccess={handleHomebrewImportSuccess}
            onDecline={handleHomebrewDecline}
          />

          {/* Browse Section */}
          <div className="space-y-3">
            <p className="text-sm text-muted-foreground">
              Vælg mappen fra din tidligere installation, backend-mappen eller databasefilen:
            </p>

            <button
              onClick={handleBrowse}
              disabled={isLoading}
              className="w-full flex items-center justify-center gap-2 px-4 py-3 min-h-[44px] bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {importState === 'selecting' || importState === 'detecting' ? (
                <>
                  <Loader2 className="h-5 w-5 animate-spin" />
                  <span>{importState === 'selecting' ? 'Vælger…' : 'Leder efter database…'}</span>
                </>
              ) : (
                <>
                  <FolderOpen className="h-5 w-5" />
                  <span>Find database</span>
                </>
              )}
            </button>
          </div>

          {/* Detection Result */}
          {detectedPath && (
            <div className="p-3 bg-success/10 border border-success/30 rounded-lg">
              <div className="flex items-start gap-2">
                <CheckCircle2 className="h-5 w-5 text-success mt-0.5 flex-shrink-0" />
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium text-success">Database fundet</p>
                  <p className="text-xs text-success mt-1 break-all">{detectedPath}</p>
                </div>
              </div>
            </div>
          )}

          {/* Error Message */}
          {importState === 'error' && errorMessage && (
            <div className="p-3 bg-destructive/10 border border-destructive/30 rounded-lg">
              <div className="flex items-start gap-2">
                <XCircle className="h-5 w-5 text-destructive mt-0.5 flex-shrink-0" />
                <div className="flex-1">
                  <p className="text-sm text-destructive">{errorMessage}</p>
                </div>
              </div>
            </div>
          )}

          {/* Action Buttons */}
          <div className="flex flex-col gap-3 pt-2">
            <button
              onClick={handleImport}
              disabled={!canImport || isLoading}
              className="w-full flex items-center justify-center gap-2 px-4 py-3 bg-success text-success-foreground rounded-lg hover:bg-success/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {importState === 'importing' ? (
                <>
                  <Loader2 className="h-5 w-5 animate-spin" />
                  <span>Importerer…</span>
                </>
              ) : importState === 'success' ? (
                <>
                  <CheckCircle2 className="h-5 w-5" />
                  <span>Færdig</span>
                </>
              ) : (
                <>
                  <Database className="h-5 w-5" />
                  <span>Importér database</span>
                </>
              )}
            </button>

            <div className="relative">
              <div className="absolute inset-0 flex items-center">
                <div className="w-full border-t border-input"></div>
              </div>
              <div className="relative flex justify-center text-sm">
                <span className="px-2 bg-card text-muted-foreground">eller</span>
              </div>
            </div>

            <button
              onClick={handleStartFresh}
              disabled={isLoading}
              className="w-full px-4 py-3 border-2 border-input text-foreground rounded-lg hover:bg-accent disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              Start forfra uden import
            </button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
