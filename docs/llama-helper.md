# llama-helper protocol and lifecycle

Tingskriver and the bundled helper exchange newline-terminated JSON on stdin/stdout.
Requests use `type: generate`, `ping`, or `shutdown`; responses use `response`
(`text`, optional `error`), `pong`, `goodbye`, or `error` (`message`). Generation
returns one complete response, not a token stream. EOF terminates the helper's
input loop. Prompts, sampling, and model selection are unchanged.

## Transaction ownership

One mutex owns both writing a request and reading its complete response. Health
pings use `try_lock`: when generation owns the connection, a ping is skipped
immediately. A ping that acquired the lock first completes within five seconds;
generation then acquires the lock. Generation's timeout includes lock acquisition,
writing, flushing, and reading. Waiting generations are serialized as well.

This deliberately retains the existing protocol without request IDs: the helper
processes requests synchronously and cannot answer a ping while generating.
Multiplexing IDs would not make that helper responsive without also moving its
model work to another thread. Skipping health pings avoids both unbounded ping
waiting and incorrectly treating a busy helper as dead. An older helper without
IDs is compatible with this serialized protocol; no ID-based matching is attempted.

The transport takes generic `AsyncRead`/`AsyncWrite` streams. It marks a transaction
interrupted before the first write and clears that state only after a complete,
valid response of the expected type (or an explicit helper error). Cancellation,
timeout, partial EOF, invalid JSON, and unexpected response types leave the
connection unusable. Subsequent requests fail with a restart-required error rather
than consuming a late response. Helper errors are returned to the caller normally.
The high-level client stops the process on user cancellation; request timeout also
stops it if the request has begun its exchange. A timeout while waiting for the
transaction lock leaves the helper and its active generation running. No response
contents are logged by the transport.

## Process and monitor lifecycle

`ensure_running` serializes model checks and spawning. Respawn stops the previous
process, cancels its monitor token, aborts and joins its old monitor tasks, then
starts one health task (30 seconds) and one idle task (60 seconds) with a fresh
`CancellationToken`. Both waits are cancellation-aware, including an in-flight
health ping. RAII guards count live health and idle tasks and decrement on drop.
Task-local manager copies do not trigger owner shutdown when dropped.

Health skips active generations. Idle shutdown skips active requests and refreshes
their activity timestamp. Graceful shutdown drains active requests for at most ten
minutes before stopping. Explicit stop cancels both monitors and kills/reaps the
helper immediately, rather than queuing a shutdown message behind generation.
The child also uses `kill_on_drop`. Stderr forwarding from PR #111 is retained.

## Windows Job Object

The Windows-only adapter lazily creates one process-wide Job Object, sets
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and assigns each spawned helper to it.
The non-inheritable job handle is held in a private `OnceLock` for the application's
lifetime. Windows closes it even when Task Manager terminates the application;
all assigned helpers are then terminated by the kernel. Temporary process handles
are closed after assignment. Job creation/configuration/assignment failures produce
a warning and leave helper startup available; kill-on-close protection is unavailable
in that failure case. Assignment occurs immediately after process creation; the
short spawn-to-assignment window is not covered. Other platforms do not use jobs.

## Automated checks

`cargo test -p tingskriver-core sidecar --lib` covers 1,000 deterministic seeded
generation/ping interleavings using `tokio::io::duplex`, health checks both before
and during generation,
cancellation, timeout, late replies, complete/partial EOF, wrong response types,
helper errors, and 20 monitor restarts with live task counters. These tests require
neither a model nor a helper binary. Windows API compilation is checked with
`cargo xwin check --target x86_64-pc-windows-msvc -p tingskriver-core`.

## Manual Windows acceptance check (about two minutes)

1. Run a Windows build containing this change and its bundled helper. Open Task
   Manager's **Details** tab and enable the PID column. Ensure no old Tingskriver
   or llama-helper process from another session is running.
2. Start a local-model generation long enough to remain active. Verify that
   `llama-helper.exe` (possibly with a target-triple suffix) is present and record
   its PID. Check the application log for Job Object warnings; any such warning
   invalidates this acceptance run.
3. While generation is active, select the Tingskriver application process in Task
   Manager and choose **End task**. Terminate only the application process, not
   the helper and not the entire process tree. Start a stopwatch at confirmation.
4. Refresh Details or watch the recorded helper PID in Process Explorer. The
   helper must disappear within **2 seconds**. Record application build/commit,
   both PIDs, observed time, and pass/fail. Repeat once after reopening the app.

For a precise timestamp, run this PowerShell observer before step 3, replacing the
two numeric placeholders with the recorded PIDs. It waits for the app to die and
measures until the helper exits, without terminating either process:

```powershell
$appProcess = Get-Process -Id 1234
$helperProcess = Get-Process -Id 5678
$appProcess.WaitForExit()
$watch = [Diagnostics.Stopwatch]::StartNew()
$gone = $helperProcess.WaitForExit(2000)
$watch.Stop()
"Helper exited: $gone; observed delay: $($watch.ElapsedMilliseconds) ms"
```

This process-kill criterion cannot be established by Linux duplex tests or a
successful Windows cross-check; it remains pending until run on Windows.
