# Server Performance And Memory Plan

This is the source of truth for making the Rust server feel like Rust again.

It is not a grab bag of micro-optimizations.
It is not a "clean things up when we touch them" note.
It is a focused plan to remove the memory growth paths, CPU churn, and avoidable copy-heavy code that have piled up during rapid development.

The standard for this work is simple:

- one Claude session and one Codex session should feel boringly stable
- long-running sessions should not accumulate surprise memory
- streaming paths should be incremental and bounded
- watcher and sync infrastructure should prefer backpressure, coalescing, and checkpoints over "just queue it"
- server state should not keep multiple oversized copies of the same payload unless there is a very good reason

## Why This Exists

The current issues compound.

A single session can hit several expensive paths at once:

- streaming text that clones growing buffers
- shell/tool output that re-broadcasts full accumulated output
- transcript sync that rereads entire files
- session snapshots that clone large diff/plan strings
- replay buffers that keep serialized event payloads in memory
- watcher infrastructure that can queue unbounded work under load

That combination is exactly how the server starts feeling "fine in isolation" but heavy in normal use.

## Non-Negotiables

- Queues must be bounded unless there is a very explicit, reviewed reason otherwise.
- Hot paths must prefer deltas, snippets, and coalescing over rebuilding full strings.
- Full transcript reparsing should be a fallback, not the default steady-state path.
- Caches and replay buffers need byte-aware limits, not just item-count limits.
- Long-lived maps keyed by path, session, repo, or request ID need explicit cleanup rules.
- We should be willing to delete and replace subsystems that are structurally wrong.

## Priority Order

### 1. Delete the rollout watcher and replace it with Codex hooks

This is still the sharpest risk, but the right move is no longer a rewrite.

The rollout watcher is a global background service with a bad cost profile for a feature we almost never use.
Keeping it alive just because it exists is how we end up preserving a major performance footgun for negligible value.

The new direction is:

- remove the rollout watcher entirely
- stop watching `~/.codex/sessions`
- stop paying the memory and CPU cost of passive file ingestion on every server boot
- move passive Codex ingestion to a hook-first server path, mirroring the Claude architecture
- accept that hook-less or legacy passive Codex support is intentionally dropped

### 2. Streaming output paths

Claude and Codex both have paths that repeatedly clone growing strings and rebroadcast the whole body.

That is a textbook "works fine at 20 lines, falls over at real usage" pattern.

### 3. Transcript sync and retained state

The current sync flow does too much full-file and full-vector work for something that should usually be incremental.

### 4. Snapshot and replay payload size

We are keeping more big strings in memory than we need, and we are reprocessing them in read paths that should stay cheap.

### 5. Secondary retention cleanup

There are a handful of smaller but still meaningful long-lived maps and buffers that need cleanup rules and better bounding.

## Replacement Plan: Provider-Agnostic Hook Architecture

### Goal

Remove the rollout watcher and replace passive Codex support with a server-first hook ingress that routes direct sessions correctly and only materializes passive sessions when hooks tell us to.

Do that in a way that gives us one shared hook architecture for Claude and Codex, without flattening their provider-specific payloads into one messy type.

### Why this is the right move

The old plan was heading toward a rewrite because the watcher is structurally bad.
The better conclusion is that we should not preserve the subsystem at all.

This is the key architectural shift:

- direct Codex sessions stay server-owned and authoritative
- passive Codex sessions should enter the system through explicit hook events, not background file discovery
- the server decides direct vs passive before materializing session state
- no global file watcher runs unless we intentionally bring back a compatibility mode later

### Scope boundary

The shared hook substrate should be provider-agnostic.

The provider adapters on top of it should stay provider-specific.

That means:

- accept Claude and Codex hook payloads through one server transport boundary
- resolve whether a hook belongs to a direct or passive session
- suppress passive shadow sessions when a direct owner exists
- cache pending passive session metadata when needed
- materialize passive sessions only when the first actionable hook arrives
- sync transcript-derived state at the right lifecycle points

It should not:

- watch `.codex/sessions`
- tail rollout JSONL files in the background
- reconstruct passive state from ambient filesystem churn
- own general session orchestration policy
- keep compatibility code for legacy passive Codex setups we no longer care about
- force Claude and Codex into one raw payload schema just because the lifecycle is shared

### What the second review confirmed

The current watcher is expensive in exactly the wrong way:

- `notify` writes into an unbounded channel in `connectors/codex_rollout/bootstrap.rs`
- each filesystem event can trigger `collect_jsonl_files(...)` rescans of directories
- each scheduled path spawns its own debounce task in `connectors/codex_rollout/runtime.rs`
- session timeouts are also separate spawned tasks keyed by session id
- file state is split across both `JsonlTailer` and `RolloutFileProcessor`
- startup seeds clone checkpoint state into both the tailer and parser

None of those are individually fatal.
Together they make burst handling and memory behavior harder to reason about than it should be.

### Architectural principles

- server ownership must be resolved before passive materialization
- direct sessions remain authoritative
- passive sessions should be event-driven, not discovery-driven
- no global watcher should exist for a niche passive path
- if hooks are unavailable, passive Codex support is unavailable by design
- reuse the Claude hook routing model instead of inventing a second architecture
- be provider-agnostic at the lifecycle layer, not at the raw payload layer

### Desired shape

#### Stage 1: Shared hook ingress

- keep one `/api/hook` ingress path
- validate and dispatch hook payloads by provider
- accept only supported hook message types per provider
- validate payloads early and fail closed on malformed events

#### Stage 2: Shared ownership routing

- resolve whether the incoming hook belongs to a direct Codex session first
- if a direct owner exists, route to that session and suppress passive materialization
- if no direct owner exists, treat it as passive
- if ownership lookup fails, suppress materialization rather than creating a shadow session

This should become a provider-neutral routing contract with provider-specific ownership lookup implementations.

#### Stage 3: Deferred passive materialization

- mirror the Claude pattern:
  - cache pending passive session metadata on session start
  - materialize only when the first actionable event arrives
- keep the passive session shape intentionally thin

This should become a shared pending-passive concept in the registry, with provider-specific pending payload structs.

#### Stage 4: Transcript and lifecycle sync

- use hook lifecycle points such as `UserPromptSubmit` and `Stop` to trigger transcript sync and passive-state updates
- derive summaries and other transcript-backed state at those explicit boundaries instead of by background file watching

### Concrete replacement shape

#### 1. Delete the watcher

- remove rollout watcher startup from server boot
- remove `connectors/codex_rollout/*`
- remove `connectors/rollout_watcher.rs`
- remove `connectors/jsonl_tailer.rs` if nothing else needs it
- remove the orphaned Codex rollout parser module once nothing references it
- remove rollout-specific persistence commands and checkpoint plumbing that only existed for watcher recovery
- drop the obsolete `rollout_checkpoints` table on upgrade

#### 2. Extract a shared hook substrate

- replace Claude-only hook ingress assumptions with provider-dispatched hook transport
- keep `/api/hook` thin: validate, dispatch, spawn, return
- define a shared routing contract for:
  - managed direct
  - passive
  - ignore shadowed by direct owner
  - ignore ownership lookup failure

#### 3. Keep provider payloads explicit

- define dedicated Codex hook client message variants instead of overloading Claude types
- keep Claude hook message variants explicit too
- do not collapse raw Claude and Codex hook payloads into one universal event bag

#### 4. Add Codex hook routing and materialization

- resolve direct ownership first
- suppress passive shadow sessions when a direct owner exists
- cache pending passive metadata until the first actionable event
- materialize passive sessions only when they become real

#### 5. Keep passive Codex intentionally small

- start with:
  - session start metadata
  - prompt submit
  - stop
  - optional Bash pre/post tool visibility later if it proves useful
- do not rebuild diff/plan/subagent/file-tail orchestration unless we prove passive Codex truly needs it

### Explicit rules

- We are not preserving backward support for hook-less passive Codex.
- We are not rewriting the watcher as a compatibility layer.
- We are not keeping a global background service around for hypothetical future use.
- The Claude hook model is the template unless Codex forces a real divergence.

### First implementation slice

The smallest worthwhile slice is:

- remove watcher startup and module wiring
- update the plan and architecture docs so the new direction is durable
- extract the shared hook ingress and routing contract
- add Codex hook message types and a thin hook ingress
- implement direct-vs-passive routing before any passive materialization happens

### Success criteria

- no rollout watcher running at server startup
- no background watching of `~/.codex/sessions`
- direct Codex sessions cannot collide with passive hook-driven sessions
- passive Codex support is event-driven and explicit
- the server gets simpler, not more abstract

## Phase 4: Streaming Output Fixes

### Claude

- stop cloning full `streaming_content` on every delta
- throttle streaming row updates
- prefer snippet/finalize semantics over full-body rebroadcast
- keep only a bounded stderr tail instead of every stderr line
- stop rebuilding aggregated per-turn patch diffs from scratch on every edit

#### Concrete changes

- introduce a Claude-side streaming throttle instead of emitting every `stream_event`
- keep the full assistant text in local state, but broadcast only on a time or growth threshold
- use a ring buffer for stderr tail instead of `Vec<String>` growth until process exit
- keep aggregated patch diff state as a mutable string or structured patch list, not `Vec<String> + join(...)` on every update

### Codex

- stop cloning full shell output on each output delta
- stop cloning full terminal interaction output on each stdin echo
- treat shell/tool output as append-only streaming state with throttled updates
- cap or summarize huge output bodies for UI transport where appropriate
- note that the generic server shell path should follow the same rules after connector hot paths are fixed

#### Concrete changes

- replace `output_buffers` full-string cloning with:
  - retained full output in memory only when needed for final result
  - throttled row updates carrying either appended chunks or capped preview text
- apply the same rule to thinking and reasoning `delta_buffers`
- review whether transport truly needs full intermediate output bodies or only the finalized full body
- apply the same bounded-channel and snippet-streaming rules to the generic shell executor used by HTTP and WebSocket shell actions

### Hot-path rules

- never clone a growing buffer on every delta unless the bounded size is tiny and explicit
- prefer one of:
  - append-only local state plus throttled preview updates
  - delta events plus finalized full state at end
  - capped preview text plus full persisted final result

### Success criteria

- long shell commands do not produce quadratic memory churn
- long assistant streams do not produce quadratic websocket payloads
- output-heavy turns remain responsive

## Phase 5: Transcript Sync And Incremental History

### Goals

- transcript sync should usually be incremental
- full transcript reparsing should be rare and intentional
- guard caches must not grow forever

### Plan

- move transcript sync toward offset- or checkpoint-based incremental reads
- avoid cloning the full transcript row vector just to build guard state
- add eviction to transcript sync guard caches
- reduce repeated parsing passes for rows and token usage when syncing the same file

### What to keep

- the idea of transcript sync guards
- provider-aware sync planning
- the ability to fall back to full resync when invariants fail

### What to change

- the current default path reparses the entire transcript file for rows, then reparses it again for token usage
- guard caching is keyed by session id with no eviction path
- guard-state calculation currently clones the full transcript rows just to compute the next guard state

### Desired steady state

- sync reads only the appended transcript region when the file grew monotonically
- rows, token usage, and turn context updates are derived in one incremental pass
- full-file parse is reserved for:
  - transcript truncation
  - missing checkpoint
  - parser mismatch
  - explicit force-resync conditions

### Concrete direction

- introduce a transcript cursor or checkpoint struct similar to the rollout watcher cursor
- persist or cache:
  - file size
  - modified time
  - offset
  - newest synced row id
  - last usage snapshot
- feed appended lines through a shared incremental transcript parser
- cap and evict `TRANSCRIPT_SYNC_GUARD_CACHE`

### Success criteria

- most syncs touch only appended transcript content
- memory cost scales with new work, not total transcript size

## Phase 6: Session Snapshot And Replay Budgeting

### Problems to fix

- session snapshots clone large `current_diff` and `current_plan` values eagerly
- dashboard reads parse diff text repeatedly
- replay buffers keep full serialized event payloads with count-based, not byte-based, limits

### Concrete direction

- precompute diff preview metadata when the diff changes, not when the dashboard asks for it
- keep heavyweight strings out of the hottest snapshot path when possible
- give replay retention a byte budget with explicit dropping rules for large payload classes
- consider excluding `DiffUpdated` from replay in favor of resync fallback if byte cost is too high

### Plan

- split heavyweight session fields from the cheap snapshot path where practical
- precompute or cache lightweight diff preview metadata at write time
- change replay retention from item count to byte budget, or exclude heavyweight event classes from replay
- audit broadcasted payloads for unnecessary duplication

### Success criteria

- dashboard/list reads stay cheap even with large diffs
- replay memory is predictable and bounded by bytes

## Phase 7: Cleanup Of Long-Lived Maps And Locks

These are smaller than the items above, but they matter over long uptime.

- add eviction or compaction for repo mutation locks keyed by repo path
- clean up any remaining long-lived per-session or per-repo state after inactivity
- audit mission orchestrator tracking maps for stale mission and issue IDs
- audit pending request maps to ensure all timeout, cancel, and exit paths remove entries

## Current Hotspots To Track

- Hook ingress still being Claude-specific instead of shared
- Claude streaming assistant text
- Codex shell output accumulation
- Terminal interaction output accumulation
- Transcript sync full-file rereads
- Session replay event log retention
- Snapshot cloning of large diff/plan payloads
- Claude per-turn patch diff aggregation
- Live log sink backpressure behavior

## Implementation Order

### Phase 1: Shared Hook Foundation

- [x] Remove rollout watcher startup and delete watcher-only modules
- [x] Extract provider-dispatched hook ingress from the current Claude-only path
- [x] Define shared hook routing outcomes and shared pending-passive session concepts
- [x] Refactor Claude hook transport onto the shared substrate without changing behavior

Worker split:
- Worker A: transport and protocol boundary
  Files:
  `orbitdock-server/crates/protocol/src/client.rs`
  `orbitdock-server/crates/server/src/connectors/hook_handler.rs`
  `orbitdock-server/crates/server/src/transport/http/router.rs`
- Worker B: shared registry and routing primitives
  Files:
  `orbitdock-server/crates/server/src/runtime/session_registry.rs`
  `orbitdock-server/crates/server/src/runtime/session_registry/connector_registry.rs`
- Worker C: Claude migration onto shared substrate
  Files:
  `orbitdock-server/crates/server/src/connectors/claude_hooks/http.rs`
  `orbitdock-server/crates/server/src/connectors/claude_hooks/handler.rs`
  `orbitdock-server/crates/server/src/connectors/claude_hooks/session_materialization.rs`

### Phase 2: Codex Hook MVP

- [x] Add Codex hook message types for `SessionStart`, `UserPromptSubmit`, and `Stop`
- [x] Extend `orbitdock hook-forward` to support Codex hook payloads
- [x] Implement direct-vs-passive Codex hook routing before materialization
- [x] Add deferred passive Codex materialization modeled after the shared hook contract
- [x] Trigger transcript/lifecycle sync on `UserPromptSubmit` and `Stop`

Worker split:
- Worker A: Codex CLI transport and forwarding
  Files:
  `orbitdock-server/crates/cli/src/cli.rs`
  `orbitdock-server/crates/cli/src/main.rs`
  `orbitdock-server/crates/server/src/admin/hook_forward.rs`
- Worker B: Codex protocol and HTTP ingress
  Files:
  `orbitdock-server/crates/protocol/src/client.rs`
  `orbitdock-server/crates/server/src/connectors/hook_handler.rs`
- Worker C: Codex runtime routing and passive materialization
  Files:
  `orbitdock-server/crates/server/src/runtime/session_registry.rs`
  `orbitdock-server/crates/server/src/connectors/codex_hooks/*`
  `orbitdock-server/crates/server/src/runtime/session_runtime_helpers.rs`

### Phase 3: Codex Hook Expansion

- [x] Persist Codex hook transcript-path updates explicitly instead of keeping them runtime-local
- [x] Add Codex `PreToolUse`, `PostToolUse`, and `PostToolUseFailure` support with Claude-parity routing
- [x] Keep Bash hook visibility lightweight, mirroring Claude hook attention state instead of projecting passive tool rows
- [x] Add ownership-collision tests for direct plus passive Codex hook traffic
- [x] Add replay and spool tests for mixed provider hook intake

### Phase 4: Streaming Output Fixes

- [x] Fix Claude streaming text updates
- [x] Fix Codex shell and terminal output streaming
- [x] Bound Claude stderr retention
- [x] Make patch diff aggregation incremental
- [ ] Fix Codex thinking and reasoning delta cloning
- [x] Apply the same bounded streaming rules to the generic server shell executor

Phase 4 progress notes:
- Claude assistant `stream_event` updates are now throttled before row rebroadcasts, so long streamed replies no longer emit a full-body `ConversationRowUpdated` for every token chunk.
- Claude stderr retention now keeps only a small tail for exit logging instead of retaining the full subprocess stderr stream in memory.
- Claude direct-edit patch aggregation now appends into one running diff string instead of rebuilding the whole aggregate with repeated `join("\n\n")`.
- Codex shell output buffering now keeps full final fidelity for completion events, but interim tool row updates only rebroadcast a bounded preview tail.
- The generic server shell transports now share the same bounded preview behavior for HTTP and WebSocket streaming updates.

### Phase 5: Transcript Sync And Incremental History

- [ ] Redesign transcript sync around incremental progress
- [ ] Add eviction to transcript sync guard caches
- [ ] Reduce duplicate transcript parsing work
- [ ] Introduce a transcript cursor or checkpoint model for steady-state sync

### Phase 6: Session Snapshot And Replay Budgeting

- [ ] Introduce byte-bounded replay retention
- [ ] Stop reparsing diffs in dashboard read paths
- [ ] Reduce heavyweight cloning in session snapshots

### Phase 7: Cleanup Of Long-Lived Maps And Caches

- [ ] Audit and bound remaining long-lived maps and caches
- [ ] Add cleanup or compaction for repo mutation locks keyed by repo path
- [ ] Add regression tests that simulate long-running sessions and output-heavy turns

## Validation Bar

We are not done when the code looks cleaner.

We are done when:

- no rollout watcher runs at startup
- Claude hooks still work after moving to the shared hook substrate
- Codex hooks land through the same server-owned hook transport
- passive Codex support does not require background file watching
- memory stays flat under long Claude and Codex streaming turns
- output-heavy shell sessions do not cause runaway allocations
- transcript sync cost scales with new data, not total history
- the server remains responsive with multiple active sessions

## Benchmarks We Should Add

- one passive Codex session driven only by hooks
- one direct Codex session running alongside passive hook traffic with no ownership collisions
- Claude and Codex hook traffic hitting the same `/api/hook` ingress without cross-provider bleed
- one long Claude streaming response
- one long Codex shell command with large stdout
- one long server shell execution streamed over HTTP or WebSocket
- mixed Claude + Codex sessions running together for an extended period
- repeated dashboard refreshes with large diffs present

## File Targets

These are the current files most likely to change in the first pass:

- `orbitdock-server/crates/protocol/src/client.rs`
- `orbitdock-server/crates/server/src/connectors/hook_handler.rs`
- `orbitdock-server/crates/server/src/connectors/codex_hooks/*`
- `orbitdock-server/crates/server/src/connectors/claude_hooks/http.rs`
- `orbitdock-server/crates/server/src/connectors/claude_hooks/handler.rs`
- `orbitdock-server/crates/server/src/connectors/claude_hooks/session_materialization.rs`
- `orbitdock-server/crates/server/src/runtime/session_registry.rs`
- `orbitdock-server/crates/server/src/runtime/session_registry/connector_registry.rs`
- `orbitdock-server/crates/server/src/admin/hook_forward.rs`
- `orbitdock-server/crates/cli/src/cli.rs`
- `orbitdock-server/crates/cli/src/main.rs`
- `orbitdock-server/crates/connector-codex/src/event_mapping/tools.rs`
- `orbitdock-server/crates/connector-codex/src/event_mapping/streaming.rs`
- `orbitdock-server/crates/connector-codex/src/runtime.rs`
- `orbitdock-server/crates/connector-claude/src/lib.rs`
- `orbitdock-server/crates/server/src/runtime/session_runtime_helpers.rs`
- `orbitdock-server/crates/server/src/infrastructure/persistence/transcripts.rs`
- `orbitdock-server/crates/server/src/domain/sessions/session.rs`
- `orbitdock-server/crates/server/src/runtime/session_registry.rs`

## Working Rule

When a subsystem is structurally wrong for performance, we should stop trying to lovingly preserve it.

The rollout watcher was the clearest example.
Delete-first, performance-principles-first, then rebuild only what still earns its keep under real OrbitDock usage.
