# Issue 70: delivery

Research: core is UI-independent; `providers::secrets::KeyStore` provides
redacted credentials. SQLite queues are the existing durable-job pattern.
There is no meeting-wide private/local policy or approved-summary type yet;
#44 is still open. GPUI settings are a placeholder; Tauri has meeting documents.

Plan:
1. Test a versioned export bundle, channel limits, privacy and durable retries.
2. Implement `delivery` core API, SQLite state, folder/HTTP/SFTP transports.
   Approval is explicit, privacy is checked again before every attempt, URLs
   live in KeyStore, remote errors never expose response bodies or addresses.
3. Expose Tauri settings/test/status and explicit approval from meeting document.
4. Publish schema and integration/manual upload-only SFTP instructions.
5. Run targeted tests, full core suite, boundary/license/Windows checks; commit
   and push milestones. PR creation/review is reserved for Mads' PC by the
   task's agent-worker instructions. Observe public CI and report exact gaps.
