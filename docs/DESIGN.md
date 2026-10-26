# Design decisions

Implementation choices that the specification left open, and what `cq` does. The full requirements and architecture are in [`SPEC.md`](SPEC.md). Back to the [README](../README.md).

## CLI and exit codes

- Exit codes are as in the table above. No general error uses exit 1, because a session treats `claim` exit 1 as "already handled". Clap's own usage errors (normally exit 2) are mapped to 64, because 2 means a rename failed.
- `cq init` creates the layout. Every other command refuses to run (exit 70) until it has. Per-name directories are created on demand.
- `CQ_ROOT` must be absolute. A relative path is refused, so that sessions in different worktrees cannot end up with different queues.
- `send` reads stdin only when it is not a terminal and `--input FILE` is not given. A leading UTF-8 BOM in the input is removed. Unknown input fields are refused (exit 64).
- `done` reads input only with `--reply` or `--reject`. With neither, it never reads stdin, so an open stdin pipe (as in Claude Code's Bash tool) cannot block it. It does check stdin without blocking, and refuses (exit 64, nothing moved) when input is waiting, so a reply given without `--reply` is never lost silently (seen live on 24 Oct 2026). Input is waiting when stdin is a pipe with bytes in it, or a non-empty file; Git Bash passes a small heredoc as a pipe and a large one as a temporary file. A console, `NUL`, a closed pipe, or an open pipe that stays empty for 100 ms counts as no input. On macOS and Linux, a non-empty file counts, and a pipe or socket counts when bytes arrive within 100 ms (Node.js, which runs Claude Code, uses socket pairs for child-process pipes). `--input` without one of the flags is refused (exit 64).
- `--reply` needs a non-empty `"reply"`; `--reject` needs a non-empty `"reason"` and allows neither `"reply"` nor `--status`. `--status` needs `--reply`. `--reply` and `--reject` cannot be combined.
- `--reply` on a `result` or `info` is refused (exit 5) before the move, so the message stays claimed. If the claimed file cannot be read, its type is unknown: `done` moves it and sends nothing.
- `cq watch` has two hidden flags for tests: `--rescan-seconds` (default 60) and `--no-fs-events`.

## Messages

- Hop limit: a message that would have 5 hops is refused, so the highest stored value is 4. With both `--parent` and `--reply-to`, hops is 1 + the higher of the two.
- Replies sent by `cq done`, and DLQ notices sent by `cq claim`, take their hop count from the message they answer, without a lookup.
- The pair, recipient and sender limits count files by name (the time stamp and sender in the ID), across the inbox, `.processing/`, `done/`, `rejected/` and `dlq/` of the recipient (pair, recipient) or of every recipient (sender). The recipient is the directory name. Messages pruned after 14 days are long outside the 1-hour window.
- The checks run in this order: arguments, body size, `PAUSE`, hops, pair, recipient, sender. The first one that fails is reported.
- The recipient limit stops many senders flooding one session; the sender limit stops one session fanning out to many. Both were added on 24 Oct 2026.
- `PAUSE` refuses sends as well as silencing watchers, so a storm stops at the source. A `done --reply` or `--reject` under `PAUSE` still moves the message and exits 4 (reply not sent). A `claim` that dead-letters a message still exits 3; its stderr line says the sender could not be told.
- The 20 KB limit is 20480 bytes of UTF-8 in `body` (and in a `done` reply or reason). The subject has no limit in the file; the event line shows at most 80 characters.
- Validation goes beyond the schema: `id` must match the file name, `to` must match the inbox, `created` must be RFC 3339 ending in `Z`, `reply_to` and `parent` must be IDs, a `result` needs a status, a `task` or `info` must have status null, and a file starting with a BOM is invalid.
- Schema: `reply_to`, `parent`, `status`, `from_session_id` and `from_host_session_id` are optional (null when absent). `cq` always writes every field.
- A DLQ notice (a `failed` result, subject `Not processed: <id>`) goes to the sender named in `from`, or, when the JSON is unreadable, to the sender named in the file's ID. It is sent for any message type, following the brief; the hop count and rate limit stop loops.
- Reply subjects: `Re: <subject>`, and `Rejected: <subject>` for a rejection. The rejection body is the reason.
- `done` checks its input first, and for `--reply` the message type, and moves nothing if either is wrong. It then moves the file and sends the reply. If the reply cannot be sent, it exits 4 with the message already in `done/` or `rejected/`.
- Reason files are `<id>.reason.json` (`id`, `inbox`, `reason`, `time`), next to the message in `dlq/` or `rejected/`.
- Moves never replace an existing file. If a send's ID is already taken, `cq` picks a new random suffix (up to 5 tries).

## Watcher

- Any file-system event in the inbox triggers a full inbox scan. The scan decides what is new, so events never need parsing. If `notify` cannot start or stops, the watcher rescans every 5 seconds and says so on stderr.
- A message is announced once per watcher run. A restarted watcher announces every unclaimed message again, once; `cq claim` exit 1 makes repeats harmless.
- Only files whose name is a valid ID are announced or listed. A file with any other name cannot be claimed.
- The event time is when the watcher saw the message, not the `created` time.
- Unreadable `from` or `type` fields show as `?` in event and list lines. Such a message still gets announced, so that `cq claim` can send it to the DLQ.
- While `PAUSE` exists, the watcher announces nothing and remembers nothing. When `PAUSE` goes, it scans at once.
- The loop ticks every 250 ms (clock, `PAUSE`). Heartbeat at start and every 30 seconds; the file holds the time. Prune at start and every hour, by file modified time.
- `claim` sets the file's modified time to the claim time, so `list --stale-minutes` measures from the claim, and the prune measures from the claim too (a move keeps the modified time).
- Lock: `File::try_lock` on `heartbeat/<name>.lock`. The OS releases it however the process exits.
- `--takeover` reads the pid file, checks that the process is a `cq` executable (same file name as the running binary), terminates it, and waits up to 5 seconds for the lock. On macOS and Linux it sends SIGTERM, then SIGKILL if the process is still running after 5 seconds. It refuses to stop any other program.
- No parent check (removed 24 Oct 2026). Under the Monitor, the parent of `cq.exe` is a forked Git Bash (the Monitor runs `bash -c "... eval 'cq watch ...' < /dev/null && pwd -P ..."`), whose lifetime Cygwin controls, not the Monitor. The check was the only path that could end a watcher with exit 0 and no output while its Monitor still ran, which was seen 4 times. `--max-seconds 1790`, the single-instance lock and `--takeover` bound an orphan instead: it lives at most 30 minutes, and `--takeover` replaces it at once.
- T14 follows from this: after only the parent is killed, the orphaned watcher keeps running and a plain second `cq watch` exits 5; `--takeover` stops the orphan, and exactly one watcher runs. A separate test runs the watcher through three nested `bash -c ... eval` forms and checks that each runs to `--max-seconds`.
- The watcher exits 0 at `--max-seconds` or when stdout can no longer be written, and prints one line to stderr saying why (`cq watch: exiting: --max-seconds 1790 reached`). Every error exit prints `cq: <reason>`. It removes the pid file if the file still holds its own ID.

## Dependencies

- clap, serde, serde_json, jiff (time zones; the bundled database on Windows), notify, windows-sys (Windows), libc (macOS and Linux). Tests: jsonschema (without default features, to avoid a network client build) and tempfile.
