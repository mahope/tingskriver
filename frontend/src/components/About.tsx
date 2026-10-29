import React, { useState, useEffect } from "react";
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { UpdateDialog } from "./UpdateDialog";
import { updateService, UpdateInfo } from '@/services/updateService';
import { Button } from './ui/button';
import { Loader2, CheckCircle2 } from 'lucide-react';
import { toast } from 'sonner';


export function About() {
    const [currentVersion, setCurrentVersion] = useState<string>('0.4.1');
    const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
    const [isChecking, setIsChecking] = useState(false);
    const [showUpdateDialog, setShowUpdateDialog] = useState(false);

    useEffect(() => {
        // Get current version on mount
        getVersion().then(setCurrentVersion).catch(console.error);
    }, []);

    const openLink = async (url: string) => {
        try {
            await invoke('open_external_url', { url });
        } catch (error) {
            console.error('Failed to open link:', error);
        }
    };

    const handleCheckForUpdates = async () => {
        setIsChecking(true);
        try {
            const info = await updateService.checkForUpdates(true);
            setUpdateInfo(info);
            if (info.available) {
                setShowUpdateDialog(true);
            } else {
                toast.success('Du har den nyeste version');
            }
        } catch (error: any) {
            console.error('Failed to check for updates:', error);
            toast.error('Kunne ikke søge efter opdateringer. Tjek din internetforbindelse og prøv igen.');
        } finally {
            setIsChecking(false);
        }
    };

    return (
        <div className="p-4 space-y-6 max-h-[80vh] overflow-y-auto">
            <div className="text-center">
                <img
                    src="/tingskriver-mark.svg"
                    alt=""
                    width={56}
                    height={56}
                    className="mx-auto mb-3"
                />
                <h2 className="text-xl font-semibold text-foreground">Tingskriver</h2>
                <p className="text-sm text-muted-foreground">Version {currentVersion}</p>
                <p className="text-sm text-foreground mt-2">
                    Referater der aldrig forlader din computer.
                </p>
                <div className="mt-4">
                    <Button
                        onClick={handleCheckForUpdates}
                        disabled={isChecking}
                        variant="outline"
                        className="min-h-[44px]"
                    >
                        {isChecking ? (
                            <>
                                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                                Søger…
                            </>
                        ) : (
                            <>
                                <CheckCircle2 className="h-4 w-4 mr-2" />
                                Søg efter opdateringer
                            </>
                        )}
                    </Button>
                    {updateInfo?.available && (
                        <p className="mt-2 text-sm text-primary">
                            Opdatering klar: version {updateInfo.version}
                        </p>
                    )}
                </div>
            </div>

            <div className="space-y-3">
                <h3 className="text-base font-semibold text-foreground">Det adskiller Tingskriver</h3>
                <dl className="space-y-3 text-sm">
                    <div>
                        <dt className="font-medium text-foreground">Lokalt fra start til slut</dt>
                        <dd className="text-muted-foreground">Optagelse, transskription og opsummering sker på din egen computer. Ingen sky, ingen lækager.</dd>
                    </div>
                    <div>
                        <dt className="font-medium text-foreground">Vælg din egen model</dt>
                        <dd className="text-muted-foreground">Brug en lokal åben model, eller tilslut en ekstern udbyder. Du er ikke låst fast.</dd>
                    </div>
                    <div>
                        <dt className="font-medium text-foreground">Ingen pris pr. minut</dt>
                        <dd className="text-muted-foreground">Kører modellerne lokalt, betaler du ikke for tid. Bruger du en ekstern udbyder, betaler du kun for de kald, du selv vælger.</dd>
                    </div>
                    <div>
                        <dt className="font-medium text-foreground">Virker med dine møder</dt>
                        <dd className="text-muted-foreground">Google Meet, Zoom og Teams, og møder uden internet.</dd>
                    </div>
                </dl>
            </div>

            <div className="pt-3 border-t border-border text-center space-y-1">
                <p className="text-xs text-muted-foreground">
                    Gratis og open source. Tingskriver er en afledning af Meetily fra Zackriya Solutions (MIT-licens), se{' '}
                    <button type="button" onClick={() => openLink('https://github.com/mahope/tingskriver/blob/main/NOTICE.md')} className="underline hover:text-foreground">NOTICE.md</button>.
                </p>
                <p className="text-xs text-muted-foreground">
                    <button type="button" onClick={() => openLink('https://tingskriver.mahoje.dk')} className="underline hover:text-foreground">tingskriver.mahoje.dk</button>
                    {' · '}
                    <button type="button" onClick={() => openLink('https://github.com/mahope/tingskriver')} className="underline hover:text-foreground">GitHub</button>
                </p>
            </div>

            {/* Update Dialog */}
            <UpdateDialog
                open={showUpdateDialog}
                onOpenChange={setShowUpdateDialog}
                updateInfo={updateInfo}
            />
        </div>

    )
}