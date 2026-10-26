# Test status

What has been tested live, and what has not. Open test work is tracked in the [GitHub issues](https://github.com/niklasrichardson/session-queue/issues?q=is%3Aissue+is%3Aopen+label%3Atesting). Back to the [README](../README.md).

## Not verified

These were not tested by `cargo test`, or not yet tested live:

- Anything on macOS. The code is checked by compiling it for macOS, but it has not run on a Mac.
- The exact cause of the 4 early watcher exits seen on 24 Oct 2026. They were not reproduced (40 runs of the Monitor's command form, 10 at a time, and 4 runs under the Monitor tool). Removing the parent check removes the only silent exit path that fits; a watcher that still ends early now prints its reason to stderr.
- That an orphaned watcher (one whose Monitor was killed) never delays work. It keeps the lock until `--max-seconds`; the new Monitor's `cq watch` exits 5 and the session re-arms with `--takeover`. In a test on 24 Oct 2026, stopping a Monitor with TaskStop killed the whole process tree, `cq.exe` included.
- The Claude Code CLI and Remote Control paths. Live tests used the desktop app's Code tab.
- The rejoin hook after a compaction or a resume. It was tested live after `/clear` only.
- Sending 25 tasks in a row with no person typing (test L3 in the spec).

Tested live in the desktop app on Windows: re-arming after each 30-minute watcher exit (hundreds of re-arms across 10 sessions), the rejoin hook after `/clear`, replies, rejections and multi-hop forwarding.

## Codex on Windows, 26 October 2026

The native dispatcher was built and tested in an isolated clone, with a separate build directory and TEMP/TMP inside that clone. No live queue was used.

- All four required checks passed: cargo build, cargo fmt --check, cargo clippy --all-targets -- -D warnings, and cargo test. The suite ran 66 tests with no failures.
- Thirteen native dispatcher integration tests use a compiled local fake Codex executable. They verify serial task handling, stdin transport, Unicode replies, no duplicate execution, sender rejection, non-task handling, invalid messages, failed/empty/oversized answers, agent rejection, timeout process-tree cleanup, unresolved completion refusal, shared watcher lock, PAUSE/resume, deferred completion, later arrivals, existing-job refusal before claim, PID/takeover, 30-second heartbeat cadence, forced dispatcher death and exit races.
- The session identity test verifies CQ_SESSION_ID precedence, empty-value fallback, sanitisation, the 64-character cap and membership lookup.
- A real read-only Codex CLI 0.162.0-alpha.2 task ran through the native cq dispatch command. It returned NATIVE CODEX ACK and delivered one correlated ok result.
- Earlier compatibility probes verified Git Bash quoted heredocs, native PowerShell 7.6.5 UTF-8 JSON pipelines, AGENTS.md loading and a bounded explicit app-server toolOutput bridge. One earlier headless run hit HTTP 429 and was reported as failed.

Not verified: native dispatcher execution on macOS/Linux, Ctrl+C handling in a foreground terminal, general tool-using write tasks, persistent app-server integration, long-running service operation, automatic recovery or exactly-once task side effects. Forced dispatcher death on Windows was tested: the kill-on-close Job Object stopped the agent and descendant. Unix SIGHUP handling compile-checked successfully for aarch64-apple-darwin; its conditional runtime test was not run here; Unix SIGKILL can still leave children. The supported worker is a fresh-process dispatcher; it does not wake an existing desktop/interactive CLI chat.

### Review follow-up

The example rules are committed as docs/SESSION-RULES.example.md; the owner's private docs/SESSION-RULES.md stays ignored. Local findings, scratch/build folders and the test queue are ignored. Public documentation links were checked against the staged file list.

Windows regression tests passed for Job Object cleanup after forced dispatcher termination, timeout cleanup, stopping an already exiting agent, PID/watch takeover, heartbeat timing, PAUSE appearing during a run, and an existing job directory leaving the message unclaimed. The test fixture is rustfmt-formatted; its executable lives in the queue test's owned TempDir and is removed with that test.

One initial full run timed out on the continuous-worker test's five-second output deadline. The test now uses a fifteen-second non-performance deadline and includes process/state diagnostics. A separate three-second watcher-readiness assertion also failed; dispatcher test startup waits now consistently allow fifteen seconds, and fixture compilation happens before starting the lock-holder watcher. The final full suite passed. Process startup also checks that ResumeThread actually resumed a suspended thread instead of accepting a no-op on a running thread; the timeout's exact cause was not established.

The existing prune test also failed with its one-second watcher lifetime. It now waits for the prune outcome under a longer bounded watcher instead of measuring startup speed. No prune behaviour was changed. A later run exposed a real concurrent-claim failure: both processes returned exit 0. Claims now hold a per-recipient OS lock across the move/validation transaction; the existing two-claim regression verifies one exit 0 and one exit 1.

A real read-only Codex acknowledgement also passed under the Windows Job Object. No changes were made to the live queue, global settings or permissions.
