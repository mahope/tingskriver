# Approved meeting delivery

Delivery is opt-in. In Tauri, open **Settings → General → Aflevering af referat**,
create a target, then disable **Kun lokal** to enable delivery. **Test mål** queues
synthetic content, never a meeting. The queue shows attempts, retries, failures
and completed deliveries. Saving a target sends nothing. On the meeting page,
save edits, choose a target, confirm consent and click **Godkend og aflever
referat**. These controls are in Tauri; GPUI's meeting/settings port is separate.

**Kun lokal** and **Privat tilstand** block every delivery, including folders
(which might be network drives), tests and retries. Existing jobs stay paused
and resume when both settings are off. Private meetings cannot be queued.
This module does not change the independently configured AI provider chain.

## Targets

- **Folder:** absolute local/mounted path. Each delivery gets a UUID subdirectory
  containing `transcript.md`, `summary.md`, `notes.md`, `sidecar.json`. The files
  are staged on the target filesystem, then the directory is renamed as a unit.
- **Generic webhook:** POST JSON with `delivery_id`, `sidecar`, `transcript`,
  `summary`, `notes`. `Idempotency-Key` is the delivery UUID. HTTPS is required
  except on loopback. Redirects are refused; only 2xx succeeds. Timeout: 30 s.
- **Slack:** JSON `text`, capped at 4,000 UTF-16 units, with unfurling disabled,
  following the [recommended text size](https://docs.slack.dev/reference/methods/chat.postMessage/).
- **Discord:** JSON `content`, capped at 2,000 UTF-16 units, with mentions
  disabled, per the [webhook API](https://github.com/discord/discord-api-docs/blob/main/developers/resources/webhook.mdx).
- **Teams channel:** incoming-webhook JSON `text`, conservatively capped at
  4,000 UTF-16 units; even JSON control-character escaping stays below the
  [28 KB limit](https://learn.microsoft.com/en-us/microsoftteams/platform/webhooks-and-connectors/how-to/add-incoming-webhook).
  Custom Teams Workflows must accept this payload or transform generic JSON.
- **SSH:** OpenSSH `sftp` on PATH, key authentication, batch mode, strict
  known-host checking, no password prompt or remote shell. Uploads four flat
  `<UUID>-<filename>` files into an existing absolute remote directory. The
  sidecar is uploaded last as a completion marker. No remote mkdir, listing,
  deletion, download or rename is requested. Timeout: 60 s. Use an unlocked
  agent or dedicated restricted key that works unattended.

Channel formats send the title and abbreviated summary. Use generic JSON,
folder or SSH for the full documents. Truncation counts UTF-16 units (including
emoji) without splitting Unicode scalars.

Webhook URLs are credentials in `providers::secrets::KeyStore` (Windows
Credential Manager in Tauri), never in configuration, queue or status responses.
The settings do not read them back. On platforms without a credential-store
implementation, saving a webhook fails closed. Folder/SSH still work. SSH key
material stays in OpenSSH key files, not the database. Errors are fixed Danish
messages, never URLs, HTTP response bodies or SSH stderr. No runtime dependency
was added.

## Queue and consent

`delivery.sqlite` uses SQLite WAL + FULL synchronous writes. Approval persists
an immutable snapshot before any attempt. Tauri processes due jobs every 15 s
and at startup. Failures back off for 60, 120, 240, 480 s; after five attempts
the job stays **failed**, with its payload intact and a manual retry action.
Interrupted **running** jobs become **pending** when the service opens.
Completed jobs retain status, but their payload is removed. Share one service
per host; do not open this queue in two application processes simultaneously.

Delivery is **at least once**: a receiver might accept data before the
connection fails, or the app might stop before recording success. JSON receivers
should deduplicate `delivery_id` / `Idempotency-Key`. Channel webhooks may show
duplicates. Folder/SSH retries reuse filenames. SSH permissions must allow
reopening an interrupted upload for writing.

A target with unfinished jobs cannot be edited or have its credential replaced.
Deleting a target removes its credential and prevents sends. Its jobs remain
visible with an error rather than disappearing.

## Core API for #44 and #105

Create one shared `delivery::Delivery::open(app_data_dir, Arc<dyn KeyStore>)`.
Use `targets()`, `save_target()`, `delete_target()`, `policy()`, `set_policy()`.
The default policy is local-only. `test_target()` queues synthetic data.
`enqueue(target_id, bundle, approved, private)` requires approval, a non-private
meeting and an enabled policy. Call `run_due()` periodically and on startup;
`jobs()` returns safe status metadata; `retry(job_id)` restarts a pending/failed
job. Every actual attempt reads current policy, not a stale approval-time copy.
A privacy change waits for the one request already in flight, then prevents
the next attempt, including other jobs in the same batch.

`Bundle` holds the documents and a `Sidecar` with `kilde: "tingskriver"`,
`skemaversion: 1`, `titel`, RFC 3339 `tid`, `talere`, and `opgaver` (`tekst`,
nullable `ansvarlig`). The published [JSON Schema](schemas/delivery-v1.schema.json)
is validated in CI with positive/negative fixtures. Rust serialization is
tested against that same fixture. Version the schema when changing its
contract. Bundles are limited to 16 MiB.

`delivery::meeting::snapshot()` adapts the existing meeting DB and canonical
notes using the exact visible summary supplied by the approval UI. Until #44
provides structured speakers/tasks, this adapter leaves those arrays empty;
it does not infer them. #44/#105 can supply a complete structured `Sidecar`
directly. Callers must propagate private-meeting state and never interpret
automatic summary generation as approval.

## Manual upload-only SFTP acceptance (Mads)

Use synthetic data and an isolated OpenSSH account. Chroot the account, make
only `/upload` writable, disable passwords, TTY, forwarding and shell access.
For example (adapt the account/chroot paths before using):

```text
Match User tingskriver-upload
    ChrootDirectory /srv/tingskriver-sftp
    ForceCommand internal-sftp -p realpath,open,write,close,lstat,stat,fstat
    AuthenticationMethods publickey
    PasswordAuthentication no
    PermitTTY no
    DisableForwarding yes
```

Keep the chroot root-owned and non-writable by the account. Use a dedicated
public key with `restrict` in `authorized_keys`. Verify/pin the server host key
out of band. Never paste a private key into settings or logs.

1. Configure host, port, account, absolute local key path and `/upload`. Enable
   delivery, press **Test mål**, confirm all four files and **Afleveret** status.
2. Verify independently that the same key cannot run a shell, read/remove files
   or forward a port. This tests the restriction as well as the upload.
3. Deny writing temporarily. A test must remain queued with an error. Restore
   writing, press **Prøv igen**, confirm successful delivery.
4. With each privacy setting on, process an already queued job and confirm no
   connection reaches the SSH/webhook listener. Turn both off and verify an
   explicitly approved synthetic meeting arrives in full.

Automated tests cover persistence, policies, real loopback webhook traffic,
schema and limits. The real restricted-server acceptance requires Mads' target.
