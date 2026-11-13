# Model downloads

Every model the app can download is pinned in
[`crates/tingskriver-core/src/model_manifest.json`](../crates/tingskriver-core/src/model_manifest.json):
engine (`whisper`, `parakeet`, `summary`), id, licence, the upstream commit (`revision`),
the sources, and for each file its exact size and SHA-256. Both speech engines and the
built-in summary models read it through `tingskriver_core::model_download`.

## What a download does

1. The file is fetched into `<name>.part` next to the final name. A `.part` file left by a
   cancel, a stall or a restart is resumed with a `Range` request.
2. Responses are checked: only a full `200`, or a `206` whose `Content-Range` matches the
   requested start and the pinned size, is accepted. A `416` restarts from zero. Extra or
   missing bytes fail the download.
3. The transfer aborts when no data arrives for 60 seconds. The `.part` file is kept.
4. The SHA-256 of the whole file must match the manifest. On a mismatch the `.part` file is
   deleted and the next source is tried; if every source fails the user gets the Danish
   error from `strings::model_hash_mismatch`.
5. Only then is the file renamed over the final name. A model that was already installed is
   never deleted or replaced by a failed, cancelled or rejected download.
6. The source that delivered the file is written to the log (`Model file X delivered by
   source 'Y'`).

Installed models are checked against the manifest before they are loaded. The hash is
computed once and remembered in `<name>.verified` (size and modification time); a changed
file is hashed again. A mismatch marks the model as corrupted and it is not loaded.

Caveat: the remembered result trusts size and modification time. A file changed in place
with the same length and a restored modification time is not hashed again. That needs
write access to the models folder, so it is not a defence against local tampering, only
against damaged or swapped downloads.

Migration: a Whisper model downloaded before pinning (from `resolve/main`, possibly another
revision) must now match the pinned size and SHA-256. If it does not, it is shown as
corrupted and the user has to download it again (up to about 3 GB for `large-v3`). This is
intentional: an unverifiable file is not loaded. The UI should explain it in the changelog
and next to the model list (see the report on #181 and #174).

## In the GPUI app (Indstillinger > Transskription, #174)

Choose the engine (Parakeet or Whisper) and see a card per model with size, source,
licence and revision; anything the manifest does not know is shown as "ukendt".

- Opening the tab only reads the models folder. A download starts when you press "Hent";
  the card shows size and source before that. Free disk space is checked first.
- "Annullér" stops a download; the partial file is kept and "Hent" resumes it. "Prøv igen"
  follows a failure (full disk, network loss, stalled source, failed checksum), each with
  a Danish explanation. A model that is already installed is never touched by a failed
  download: core swaps a file in only after its SHA-256 matches.
- "Kontrolsum (SHA-256) bekræftet" appears only after a download that core checked, or
  after "Kontrollér" has hashed the installed files. Otherwise the card says it has not
  been checked in this session. A model that fails the check is offered "Slet" and "Hent igen".
- "Vælg" saves the choice in `transcript_settings`, the row the Tauri app reads.
- "Slet" asks first. It is disabled, with the reason under the card, while a recording,
  its final transcription, a dictation or a transcription job is running. The check is
  made again when you press "Slet" in the dialog, so a recording started meanwhile still
  blocks the delete.

Limits: the cards do not show warm/idle state or GPU/CPU status, and Syv.ai is a dictation
engine set on the Diktering tab. Cloud providers that the Tauri app can save (Deepgram,
ElevenLabs, Groq, OpenAI) are not offered here. If one is saved, the tab names it under
the cards and warns that choosing a local model replaces it. Android downloads stay in #134.

## Sources and the reserve mirror

`sources` is tried in order: the first is the upstream repository pinned to `revision`.
Further entries are reserve mirrors, plain URLs with the same file names. The
`github-release` entries point at the release `modeller-v1` of this repository. **The
release `modeller-v1` does not exist yet (checked 2026-11-05: 404), so the mirror is not
live until someone with release rights creates it and uploads the assets** (same file names as in the
manifest, or `remote_name` for files whose upstream name differs). Until then the mirror
answers 404 and the download simply reports that no source delivered the file. The mirror
is only contacted after the first source has failed. There is no telemetry.

## Updating or adding a model

Download the file with `curl` from the pinned URL, compute `sha256sum` and the size,
and put them in the manifest together with the repository commit. Do not copy numbers from
a web page. The manifest test in `model_download.rs` checks that every Whisper model in
`config::WHISPER_MODEL_CATALOG` and both Parakeet models are listed, that the first source
is pinned to the revision, and that every hash is well formed.

Licences: Whisper models MIT (whisper.cpp), Parakeet CC-BY-4.0, Qwen Apache-2.0, Gemma
under the Gemma terms. The model files keep their own licences.
