# cq commands

Every command, its exit codes and the files it uses. Back to the [README](../README.md).

## Commands

Every command has `--help`. Message text never goes on the command line: `send` reads JSON from stdin (use a quoted heredoc, `<<'EOF'`) or from `--input FILE`. `done` reads JSON the same way, but only with `--reply` or `--reject`; without them it refuses input on stdin (exit 64, nothing moved).

| Command | What it does |
| --- | --- |
| `cq init` | Create the layout under `CQ_ROOT`. |
| `cq send --from A --to B --type task\|result\|info [--reply-to ID] [--parent ID] [--status ok\|failed\|rejected] [--input FILE]` | Read `{"subject": "...", "body": "..."}`, write the message, print its ID. |
| `cq watch --as B [--takeover] [--max-seconds 1790]` | Print one `CQ-NEW` line per new message. One watcher per name. |
| `cq claim --as B ID` | Move the message to `.processing/` and print its JSON. |
| `cq done --as B ID [--reply [--status ok\|failed] \| --reject] [--input FILE]` | Finish a claimed message. With no flag, sends no reply, and refuses (exit 64, nothing moved) if input is waiting on stdin. `--reply` reads `{"reply": "..."}` (tasks only); `--reject` reads `{"reason": "..."}`. |
| `cq list --as B [--stale-minutes 60]` | List unclaimed, stale claimed and dead-lettered messages, oldest first. No bodies. |
| `cq leave --as B` | Forget which session owns `B` (remove its `members/` record). Exit 0 when there is none. |
| `cq members` | Print one `<session key> -> <name>` line per record. |
| `cq session-start [--debug]` | For the SessionStart hook. Prints the rejoin reminder when this session owns a name, else nothing. Always exits 0. |

Example:

```bash
cq send --from lead --to worker-a --type task <<'EOF'
{"subject": "Run the tests", "body": "Run `cargo test` in D:/Code/x and report failures."}
EOF
```

Reply to a task, or reject a message:

```bash
cq done --as worker-a 20261024T000615Z-lead-ab12 --reply <<'EOF'
{"reply": "cargo test passed."}
EOF

cq done --as worker-a 20261024T000615Z-lead-ab12 --reject <<'EOF'
{"reason": "Deploys are out of scope for this repo."}
EOF
```

Event line:

```text
CQ-NEW 2026-10-24T00:06:16Z (00:06 GMT) to=worker-a from=lead type=task id=20261024T000615Z-lead-ab12 subject="Run the tests"
```

The time is when the watcher saw the message: UTC first, then local time with its label, computed by `cq` (see `CQ_TZ` and `CQ_TZ_LABEL` in [Setup](SETUP.md#configuration)).

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | OK |
| 1 | `claim`: the message is no longer in the inbox (another claim won, or a duplicate event). Ignore it. |
| 2 | A rename still failed after 3 retries over 1 second. |
| 3 | `claim`: the message is invalid and went to `dlq/`. The sender was told, when known. |
| 4 | `done`: the message is finished, but the reply was not sent. |
| 5 | Refused: bad name, body over 20 KB, hop limit, a pair, recipient or sender limit, `PAUSE`, missing parent, a watcher already running, or `done --reply` on a `result` or `info`. |
| 6 | `done`: the message is not claimed by this session. |
| 64 | Bad arguments or bad JSON input, or `done` input without `--reply` or `--reject`. |
| 70 | Any other error (I/O, `CQ_ROOT` missing or not initialised). |


## Layout

```text
CQ_ROOT/
  tmp/                       temp files, renamed into inboxes
  inbox/<name>/              new messages
  inbox/<name>/.processing/  claimed messages
  done/<name>/               finished messages (pruned after 14 days)
  rejected/<name>/           declined messages and reason files (pruned after 14 days)
  dlq/<name>/                invalid messages and reason files (never pruned)
  heartbeat/<name>           time of the watcher's last heartbeat (every 30 s)
  heartbeat/<name>.lock      locked by the running watcher
  heartbeat/<name>.pid       the running watcher's process ID
  members/<key>              the name owned by a Claude Code session, and the UTC time it was recorded
  PAUSE                      while it exists, watchers print nothing and cq send refuses
```

Message format: [`schema/message.v1.json`](../schema/message.v1.json). One file per message, `<id>.json`, UTF-8 without BOM. IDs are `<UTC yyyymmddThhmmssZ>-<from>-<4 hex>`. Readers ignore unknown fields.

To stop all queue traffic at once, create a file named `PAUSE` in the queue root (for example `touch "$CQ_ROOT/PAUSE"`). While it exists, watchers announce nothing and every `cq send` is refused (exit 5). Remove the file to resume; waiting messages are then announced.
