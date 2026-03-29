# Control Deck Architecture Plan

This is the source of truth for building `Control Deck` correctly.

It is not a migration plan.
It is not a "replace the composer gradually" plan.
It is a clean-slate architecture plan for a new input surface with a server-owned contract and UI components that are not coupled to transport models.

The current composer remains the live surface while Control Deck is built alongside it in its own files.
There are no feature flags and no temporary host wrapper.
As soon as Control Deck is semi-usable, we will wire it in directly and start dogfooding it.
After that, we iterate in the live path and delete the old composer once the new deck has proven itself.

## Non-Negotiables

- The Rust server owns durable Control Deck truth.
- HTTP owns bootstrap and mutations.
- WebSocket, if used, only carries light deltas, replay, and refetch hints.
- SwiftUI views do not depend directly on `Server...` transport models.
- SwiftUI views do not build server requests directly.
- The Control Deck is its own component system and domain model.
- We do not architect around the legacy composer.
- It is acceptable to throw away prototype code that does not fit this plan.
- Control Deck should not be mounted into the live footer until it meets the architecture and product bar.

## What "Done Right" Means

The new Control Deck should have:

- one authoritative bootstrap payload for the surface
- one authoritative submit contract for a turn
- one authoritative preferences contract for durable deck configuration
- one clear client domain layer between transport and UI
- one clear encoding boundary between client domain and submit request
- zero transport-type leakage into reusable UI components
- a visual baseline that preserves the best parts of the current composer footer instead of resetting the design language

If a view needs `ServerControlDeckSnapshotPayload`, `ServerControlDeckAttachmentRef`, or another server transport type directly, we are not done.

## Visual Baseline

Control Deck is not a mandate to visually replace everything the current composer already does well.

We should preserve and rebuild the strongest parts of the existing footer experience:

- the dense, glanceable bottom status strip
- the pill-based status language for autonomy and working mode
- the compact metadata labels for model, effort, branch, cwd, and context usage
- the integrated feeling between attachments, input, controls, and status

We can redesign the structure and component boundaries, but we should not drift into generic stacked cards or dashboard-like framing.

If the new deck feels calmer, denser, and more OrbitDock than the current composer, we are on the right track.
If it feels like a generic panel with an editor dropped into it, we are off course.

## Target Server Surface

### 1. Snapshot

`GET /api/sessions/{session_id}/control-deck`

Returns a single Control Deck bootstrap contract:

- session-facing state needed to render the deck
- provider capabilities needed to enable or disable features
- initial preferences needed to shape the surface
- surface revision / refresh semantics

This is the only required bootstrap endpoint for the deck itself.

### 2. Submit

`POST /api/sessions/{session_id}/control-deck/submit`

This is the only turn submission endpoint for the deck.

The request must be a typed Control Deck contract, not an adaptation of old `sendMessage` bags.

### 3. Preferences

`GET /api/control-deck/preferences`
`PUT /api/control-deck/preferences`

This is the durable settings surface for:

- status bar modules
- ordering
- density
- future deck-level preferences

### 4. Attachments

Attachments should be treated as part of the Control Deck contract, not as generic old-composer helpers.

V1 can temporarily reuse the existing session image upload path at the transport level if needed, but the Control Deck architecture should model attachments as belonging to the deck.

That means:

- the deck owns attachment intent
- the client domain owns attachment draft state
- the submit contract owns attachment references

If attachment handling feels like "reaching out to some random old endpoint," the plan is not satisfied.

## Target Server Models

These protocol types are the right direction and should remain server-owned:

- `ControlDeckSnapshot`
- `ControlDeckState`
- `ControlDeckCapabilities`
- `ControlDeckPreferences`
- `ControlDeckModulePreference`
- `ControlDeckSubmitTurnRequest`
- `ControlDeckAttachmentRef`
- `ControlDeckMentionRef`
- `ControlDeckImageAttachmentRef`
- `ControlDeckSkillRef`
- `ControlDeckTurnOverrides`

But the client should not spread these through the UI tree.

## Required Client Layers

The client must have three distinct layers:

### 1. Transport Layer

Lives under `Services/Server/...`

Responsibilities:

- encode/decode HTTP contracts
- call server endpoints
- nothing about UI presentation

Examples:

- `ControlDeckClient`
- `ControlDeckContracts`

### 2. Client Domain Layer

Lives under the new Control Deck feature, but separate from views.

Responsibilities:

- map transport models into Control Deck-native client models
- own local draft state
- own local completion state
- own local attachment draft state
- own presentation-ready state for the view tree
- encode client draft state back into the submit request at exactly one boundary

Required models:

- `ControlDeckSnapshotModel`
- `ControlDeckPresentation`
- `ControlDeckDraft`
- `ControlDeckAttachmentState`
- `ControlDeckCompletionState`
- `ControlDeckSubmitDraft`

Important rule:

- these are not aliases of `Server...` types

### 3. View Layer

Lives under `Views/Sessions/ControlDeck/`

Responsibilities:

- render UI from client domain and presentation models
- emit user intents
- never know transport shapes

Important rule:

- no reusable Control Deck view should accept `Server...` models directly

## Target UI Architecture

The Control Deck should be composed from explicit, isolated components.

### Top-level shell

- `ControlDeckView`
- `ControlDeckScreen`

### Core visual components

- `ControlDeckHeader`
- `ControlDeckDraftEditor`
- `ControlDeckAttachmentTray`
- `ControlDeckCompletionPanel`
- `ControlDeckSubmitBar`
- `ControlDeckStatusBar`

### Settings / configuration components

- `ControlDeckPreferencesSection`
- `ControlDeckModulePicker`

### Rules

- `ControlDeckView` coordinates feature state and intent routing
- leaf views consume deck-native models only
- submit UI is its own component, not fused into the editor
- attachment UI is its own component, not fused into the editor
- completion UI is its own component, not fused into the editor

## Current Prototype Audit

This section is intentionally blunt.

### Keep

- The Control Deck protocol types on the server.
- The dedicated `GET /control-deck`, `POST /control-deck/submit`, and preferences endpoints.
- The dedicated `Views/Sessions/ControlDeck/` tree.
- The new shell-level component names and design direction.

### Rework

- `ControlDeckViewModel`
  Reason: it still exposes raw `Server...` contracts directly to the view layer.

- `ControlDeckView`
  Reason: it still derives presentation from transport payloads and handles request-shape concerns directly.

- `ControlDeckAttachmentState`
  Reason: it still stores `ServerControlDeckAttachmentRef` directly.

- `ControlDeckDraft`
  Reason: it still builds `ServerControlDeckSubmitTurnRequest` directly.

### Delete / Replace If Needed

- Any helper or component that exists only to pass `Server...` types deeper into the view tree.
- Any "quick fix" state that duplicates server contract meaning instead of introducing a proper client domain model.

## Gap Between Current State And Target

Today, the prototype still has these architecture violations:

- raw transport payloads are read directly in the top-level Control Deck view
- UI-facing local state still stores transport-layer attachment refs
- the draft model still constructs the wire request directly
- image import/upload logic lives in the view instead of behind a clearer deck intent boundary

That means the current prototype is useful as:

- design exploration
- endpoint validation
- component naming exploration

But it is not yet the correct architecture.

## Revised Phases

## Phase 1: Lock The Contract

- [x] Define additive server protocol types.
- [x] Add dedicated Control Deck HTTP endpoints.
- [x] Remove architectural language that treats this as a migration.
- [x] Decide whether attachment upload becomes a dedicated Control Deck endpoint now or remains a narrow reused transport implementation behind the deck boundary.
- [ ] Write down the exact bootstrap payload fields the Control Deck UI needs from `GET /control-deck`
- [ ] Write down the exact submit payload shape the client domain is allowed to encode
- [ ] Write down the exact preferences fields that are durable versus view-local
- [x] Move snapshot shaping, capability calculation, default preferences, and submit validation out of the HTTP handler and into a dedicated Control Deck domain/runtime service
- [ ] Replace the current `submit_control_deck_turn` adaptation over `dispatch_send_message` with a Control Deck-specific runtime entrypoint
- [x] Make `ControlDeckClient` the sole Swift transport entrypoint for deck behavior, including attachment transport if it is reused behind the deck boundary
- [ ] Add server tests that lock bootstrap fields, submit rules, and attachment / mention semantics

## Phase 2: Build The Client Domain Layer

- [x] Add `ControlDeckSnapshotModel`
- [x] Add `ControlDeckPresentation`
- [x] Add `ControlDeckDraft`
- [x] Add `ControlDeckAttachmentState`
- [x] Add `ControlDeckCompletionState`
- [x] Add one explicit mapper from server snapshot to client snapshot/presentation
- [x] Add one explicit encoder from client draft state to submit request
- [x] Remove direct `Server...` types from reusable Control Deck view APIs
- [x] Add `ControlDeckSubmitDraft`
- [x] Add `ControlDeckSnapshotMapper.swift` as the only inbound server-to-domain mapping boundary
- [x] Add `ControlDeckSubmitRequestEncoder.swift` as the only outbound domain-to-server encoding boundary
- [x] Replace `Server...` storage inside `ControlDeckViewModel` with deck-native models only
- [x] Replace transport-coupled attachment state with deck-native attachment items that track semantic type, local ID, preview metadata, and upload lifecycle
- [x] Run a phase audit: no reusable Control Deck view signature should mention `ServerControlDeck...`

This is the most important phase.
We should not keep building visual features until this exists.

## Phase 3: Rebuild The Core Deck UI On The Domain Layer

- [x] Rebuild `ControlDeckView` to consume client presentation models only
- [x] Rebuild `ControlDeckViewModel` around deck-native state and intents
- [x] Add a `ControlDeckScreen` shell that owns loading, errors, refresh, and bootstrap orchestration
- [ ] Keep `ControlDeckHeader`
- [ ] Keep or rewrite `ControlDeckDraftEditor` depending on fit
- [ ] Keep or rewrite `ControlDeckAttachmentTray` depending on fit
- [x] Add a dedicated `ControlDeckSubmitBar`
- [x] Ensure no leaf view accepts transport models
- [ ] Delete or replace `ControlDeckEditorSection` if it remains a mixed editor + attachment + submit + status bag
- [x] Move `ControlDeckStatusModulePresentation` out of the view layer and feed it from deck-native snapshot / presentation models
- [ ] Keep `ControlDeckHeader`, `ControlDeckDraftEditor`, `ControlDeckAttachmentTray`, and `ControlDeckStatusBar` only if they remain transport-free leaf components

## Phase 4: Attachments And Mentions

- [x] Add deck-native image attachment draft flow
- [ ] Add deck-native file mention draft flow
- [ ] Stop relying on raw filename substitution completely
- [ ] Move to typed mention identity end-to-end
- [ ] Validate mention semantics on the server

## Phase 5: Completion System

- [ ] Add `ControlDeckCompletionState`
- [ ] Add `ControlDeckCompletionPanel`
- [ ] Add skill completion against deck-native models
- [ ] Add file completion against deck-native models
- [ ] Add image-aware affordances without overriding real completion behavior

## Phase 6: Preferences And Status Bar

- [ ] Make status bar rendering fully preference-driven
- [ ] Add ordered and toggleable modules
- [ ] Add Control Deck settings UI
- [ ] Keep durable preferences server-backed
- [ ] Keep purely visual local concerns device-local

## Phase 7: Early Dogfooding

- [ ] Wire Control Deck into the live footer as soon as the core send loop, attachments, and basic status UI are usable
- [ ] Start using it daily before full parity
- [ ] Capture paper cuts directly against the new surface
- [ ] Iterate on the real product path instead of polishing forever off to the side
- [ ] Use this exact semi-usable bar for the switch:
  - [ ] bootstrap from one Control Deck snapshot
  - [ ] reliable plain-text send
  - [ ] reliable image attach and send
  - [ ] image paste/import works from the draft surface
  - [ ] draft preservation on failures
  - [ ] basic status bar rendering from deck-native presentation models
  - [ ] usable loading, error, and disabled-input states
  - [ ] at least one real autocomplete path exists in the live editor
  - [ ] the live input ergonomics are not materially worse than the old composer

## Phase 8: Evaluation

- [ ] Audit whether any Control Deck view still knows about transport models
- [ ] Audit whether any Control Deck model still acts as a disguised server DTO
- [ ] Audit whether the deck still depends on scattered non-deck endpoints semantically
- [ ] Delete or rewrite anything that fails the audit

## Immediate Next Step

Before more feature work, do this:

- [x] Introduce deck-native client models that wrap the current server snapshot, draft, attachment, and presentation concepts
- [x] Refactor `ControlDeckViewModel` to expose those models instead of `Server...` types
- [x] Refactor `ControlDeckView` and its child views to consume only deck-native models
- [ ] Re-evaluate the current prototype files and throw away the pieces that do not fit

## Ready-To-Start Work Items

- [x] Define `ControlDeckSnapshotModel` and `ControlDeckPresentation` in the Control Deck feature
- [x] Define `ControlDeckDraft`, `ControlDeckAttachmentState`, and `ControlDeckCompletionState` as deck-native client models
- [x] Add one mapper from `ServerControlDeckSnapshotPayload` to `ControlDeckSnapshotModel`
- [x] Add one encoder from `ControlDeckDraft` to `ServerControlDeckSubmitTurnRequest`
- [x] Rewrite `ControlDeckViewModel` so views never read `Server...` types directly
- [x] Rewrite `ControlDeckView` into a screen-level shell only
- [x] Add `ControlDeckScreen` and keep leaf components transport-free
- [x] Decide attachment upload direction: dedicated Control Deck transport or narrow reused upload path behind the deck boundary

## Evaluation Standard

At the end of the next pass, we should be able to say:

- the server contract is coherent
- the client domain is coherent
- the UI is not coupled to transport
- the deck is a real product surface, not a cleaned-up wrapper around old behavior
