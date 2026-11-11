# App changelogs

Commit `docs/changelog/<version>.md` before tagging `app-v<version>`.
Use UTF-8 Markdown: a version heading and concise user-facing changes with PR
references. The whole file becomes the GitHub release body, byte for byte;
do not add a second copy of the release text to a workflow.

Both stable versions and pre-releases require this file. Auto release writes
its own file for each dev version. For a stable release, also commit
`<version>.performance.json`, the approved reference-machine measurement in
[the performance gate format](../ydeevne.md). Never include personal data,
credentials or private machine names. Missing measurements or an unmeasured
baseline block stable publication. See [the release gates](../ci.md).
