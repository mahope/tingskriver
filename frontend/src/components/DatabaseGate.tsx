'use client'

import { useEffect, useState, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { Button } from '@/components/ui/button'

interface DbFailureInfo {
  detail: string
  database_path: string
  backup_path: string | null
  log_path: string | null
}

// Mirrors `DatabaseStatus` in src-tauri/src/database/setup.rs.
type DatabaseStatus =
  | { state: 'loading' }
  | { state: 'first_launch' }
  | { state: 'ready' }
  | {
      state: 'failed'
      error: { kind: 'locked' | 'corrupt' | 'disk_full' | 'migration' | 'other'; info: DbFailureInfo }
      summary: string
      advice: string
    }
  | { state: 'error'; message: string }

// Show the loading text only when opening takes noticeably long, so a
// normal start does not flash it.
const LOADING_TEXT_DELAY_MS = 400

/**
 * Renders the app once the database has opened in the background (or there
 * is none yet, before onboarding). Until then a quiet loading state, and if
 * the database fails to open an error page with the log and the backup copy,
 * instead of the app exiting without a word.
 */
export function DatabaseGate({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<DatabaseStatus>({ state: 'loading' })
  const [showLoadingText, setShowLoadingText] = useState(false)

  useEffect(() => {
    let cancelled = false
    const unlisten = listen<DatabaseStatus>('database-status', (event) => {
      if (!cancelled) setStatus(event.payload)
    })
    // Ask once subscribed, so a status sent in between is not missed.
    unlisten
      .catch(() => undefined)
      .then(() => invoke<DatabaseStatus>('get_database_status'))
      .then((current) => {
        if (!cancelled && current.state !== 'loading') setStatus(current)
      })
      .catch((error) => {
        // Outside Tauri (plain `next dev`) there is no backend to wait for.
        console.warn('[DatabaseGate] get_database_status failed:', error)
        if (!cancelled) setStatus({ state: 'ready' })
      })
    const timer = setTimeout(() => setShowLoadingText(true), LOADING_TEXT_DELAY_MS)
    return () => {
      cancelled = true
      clearTimeout(timer)
      unlisten.then((fn) => fn())
    }
  }, [])

  if (status.state === 'ready' || status.state === 'first_launch') {
    return <>{children}</>
  }

  if (status.state === 'loading') {
    return (
      <div className="flex h-screen items-center justify-center bg-background" aria-busy="true">
        {showLoadingText && (
          <p className="text-sm text-muted-foreground" role="status">
            Åbner dine møder…
          </p>
        )}
      </div>
    )
  }

  return <DatabaseErrorPage status={status} />
}

function DatabaseErrorPage({
  status,
}: {
  status: Extract<DatabaseStatus, { state: 'failed' } | { state: 'error' }>
}) {
  const [actionError, setActionError] = useState<string | null>(null)
  const failed = status.state === 'failed' ? status : null
  const info = failed?.error.info

  const run = (command: string) => {
    setActionError(null)
    invoke(command).catch((error) => setActionError(String(error)))
  }

  return (
    <div className="flex h-screen items-center justify-center bg-background p-8 text-foreground">
      <div className="w-full max-w-xl space-y-5" role="alert">
        <h1 className="font-serif text-2xl">
          {failed ? failed.summary : 'Tingskriver kunne ikke starte.'}
        </h1>
        <p className="text-base leading-relaxed">
          {failed
            ? failed.advice
            : 'Start Tingskriver igen. Sker det igen, så send logfilen til udvikleren.'}
        </p>

        <dl className="space-y-2 text-sm text-muted-foreground">
          {info && (
            <div>
              <dt className="font-medium text-foreground">Databasefil</dt>
              <dd className="break-all font-mono text-xs">{info.database_path}</dd>
            </div>
          )}
          {info?.backup_path && (
            <div>
              <dt className="font-medium text-foreground">Kopi af databasen</dt>
              <dd className="break-all font-mono text-xs">{info.backup_path}</dd>
            </div>
          )}
          {info?.log_path && (
            <div>
              <dt className="font-medium text-foreground">Logfil</dt>
              <dd className="break-all font-mono text-xs">{info.log_path}</dd>
            </div>
          )}
          <div>
            <dt className="font-medium text-foreground">Teknisk fejl</dt>
            <dd className="break-words font-mono text-xs">
              {failed ? failed.error.info.detail : status.state === 'error' ? status.message : ''}
            </dd>
          </div>
        </dl>

        <div className="flex flex-wrap gap-3">
          <Button onClick={() => run('open_log_folder')}>Åbn logmappen</Button>
          {info?.backup_path && (
            <Button variant="outline" onClick={() => run('open_database_backup_folder')}>
              Åbn backup
            </Button>
          )}
        </div>
        {actionError && <p className="text-sm text-destructive">{actionError}</p>}
      </div>
    </div>
  )
}
