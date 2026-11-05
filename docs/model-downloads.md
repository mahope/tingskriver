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

## Sources and the reserve mirror

`sources` is tried in order: the first is the upstream repository pinned to `revision`.
Further entries are reserve mirrors, plain URLs with the same file names. The
`github-release` entries point at the release `modeller-v1` of this repository. **The
release assets must be uploaded for that mirror to work** (same file names as in the
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
