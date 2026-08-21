# Grimoire — Architecture

> The Library holds the work.
> The Volume gives it identity.
> The Chapter gives it structure.
> The Page is where writing happens.
> The Margin is where thinking happens.
> AI belongs in the Margin.
> The manuscript belongs to the user.

## 1. Shape of the system

Grimoire is a single-process desktop application. There is no server, no sync, no account.

```
┌───────────────────────────────────────────────────────────┐
│ Tauri 2 host process (Rust)                               │
│                                                           │
│  commands/      thin IPC surface, one module per concern  │
│  repositories/  all SQL; owns transactions                │
│  domain/        entities + invariants; no I/O             │
│  database/      pool, migrations, FTS5 triggers           │
│  ai/            provider trait, OpenAI-compatible impl    │
│  credentials/   OS keychain; DB stores only a reference   │
│  backup/        archive, snapshot, import/export          │
│                                                           │
│         ▲ invoke / event stream                           │
│         │                                                 │
│  ┌──────┴────────────────────────────────────────────┐    │
│  │ WebView (Svelte 5 + TypeScript + Vite)            │    │
│  │                                                   │    │
│  │  routes/      library, workspace, settings        │    │
│  │  editor/      Tiptap schema, commands, autosave   │    │
│  │  manuscript/  navigation tree                     │    │
│  │  annotations/ margin                              │    │
│  │  services/    typed wrappers over `invoke`        │    │
│  │  stores/      runes-based state, one owner each   │    │
│  │  design/      tokens, themes, primitives          │    │
│  └───────────────────────────────────────────────────┘    │
└───────────────────────────────────────────────────────────┘
                          │
                   SQLite (FTS5) on local disk
```

## 2. Where logic lives

The boundary rule is: **Rust owns durability, the WebView owns interaction.**

Formatting lives over the selection rather than in a permanent strip. A toolbar across the
top spends the most valuable row on screen on controls that are idle most of the time, and
makes a writing application look like a word processor.

| Concern | Owner | Rationale |
| --- | --- | --- |
| Rich text editing, selection, IME | WebView | Browsers are the best text editors ever built. |
| Layout, responsiveness, motion | WebView | CSS is the right tool. |
| Document JSON shape | Shared contract | ProseMirror defines it; Rust stores it opaquely. |
| SQL, transactions, migrations | Rust | Correctness and crash safety must not depend on a WebView being alive. |
| Word/character counts | Rust | Derived once, at the point of persistence, so they cannot drift. |
| Anchor relocation | Rust | Data-safety-sensitive; must be unit-testable without a DOM. |
| AI HTTP transport, streaming, cancellation | Rust | Keeps API keys out of the WebView entirely. |
| Prompt templates | Rust | Centralised; never scattered through components. |
| Credential storage | Rust | OS keychain access is native-only. |

Two rules follow from this and are enforced by review:

1. **Rust never does presentation work.** It returns data, not formatted strings, not
   class names, not colours.
2. **Svelte never does durability work.** A component may not decide when something is
   safe to overwrite. It asks; Rust decides.

### The commands layer is not an RPC dump

`commands/` is deliberately organised by domain concern (`volumes`, `chapters`, `pages`,
`annotations`, `search`, `ai`, `data`), and each command is a thin adapter: deserialize →
call a repository or service → map error. Business rules live in `domain/` and
`repositories/`, never in a command body. If a command grows past a few lines of glue,
the logic belongs one layer down.

## 3. Data model

The canonical page representation is **ProseMirror document JSON**, stored as text in
`pages.document_json`. Markdown is an import/export format only — round-tripping through
Markdown would silently destroy structure the schema supports.

`pages.plain_text` is a derived projection maintained by Rust in the same transaction as
`document_json`. It exists for search, previews, statistics, and AI context. Because it is
written transactionally alongside its source, it cannot drift out of sync.

Every persistent entity carries a UUID primary key. Ordering within a parent uses an
integer `position` normalised on every structural mutation, inside a transaction.

### Revisions

Revisions are snapshots, not diffs — the simplicity is worth the disk. They are created at
meaningful checkpoints, periodically during long sessions, and unconditionally before any
destructive operation (restoring another revision, applying an AI suggestion). This is not
version control and will not grow into it; history is bounded per page.

### Annotation anchoring

An anchor stores `from`/`to` offsets plus `selected_text`, `context_before`,
`context_after`, the `base_revision` it was created against, and a hash of the anchored
text. When a page changes, Rust attempts relocation: exact match first, then context-
window match, then fuzzy match. Below a confidence threshold the annotation is marked
**stale** rather than relocated.

An annotation that cannot be placed confidently is never placed. Pointing a comment at the
wrong sentence is worse than admitting the target is gone.

### AI suggestions

A suggestion persists `page_id`, `base_revision`, its anchor, `original_text`,
`replacement_text`, and a `context_hash`. **Apply re-validates against live content.** If
the source text no longer matches, the patch is refused and the suggestion is marked
stale; the user may then inspect, re-evaluate, or dismiss it. There is no code path that
applies a suggestion to text it was not computed against.

## 4. Responsive information architecture

Breakpoints change *what exists*, not merely how wide it is.

| State | Width | Navigation | Editor | Margin |
| --- | --- | --- | --- | --- |
| Wide | ≥ 1180px | persistent pane | centred column | persistent pane |
| Medium | 820–1179px | persistent pane | centred column | slide-over |
| Narrow | < 820px | overlay | full width | overlay |

Surface Go portrait (≈768 CSS px at 150% scaling) lands in Narrow; landscape (≈1024) lands
in Medium. Both are primary targets, not degraded fallbacks.

The Margin is not a sidebar. Where it is a pane, it shares one scroll container with the
manuscript and each anchored note is positioned level with the words it refers to; notes
that would collide are pushed down only as far as they must go to clear the one above.
That vertical relationship is the whole difference between a marginal note and a comment
in a panel, and it is why the notes are laid out in a single pass from the sheet's own
origin rather than in normal flow.

Where the Margin can only be an overlay there is nothing beside it to align to, so notes
fall back to reading order — the alignment belongs to sitting next to the text, not to the
note. The pane's open state is remembered; the overlay's is not, because restoring it
would cover the writer's text with notes the moment they opened the app.

Touch targets are ≥ 40px. No action is reachable only by hover, drag, or right-click —
gestures are accelerators layered over controls that already exist.

## 5. Performance stance

The reference device is a low-power tablet, so:

- No database work happens synchronously in the typing path. Ever.
- Autosave is debounced and queued; a save in flight never blocks a keystroke.
- AI streaming is delivered as Tauri events and never blocks the UI thread.
- Long lists are virtualised; large libraries lazy-load.
- Search is FTS5-indexed, maintained by triggers, never a `LIKE` scan.

## 6. Privacy stance

Local, private, user-owned, by default and without a setting to change it:

- No analytics, no telemetry, no crash reporting to any third party.
- Manuscript text leaves the machine only when the user explicitly invokes an AI action,
  and only the context that action requires — budgeted, never the whole Volume.
- API keys live in the OS credential store. The database holds a reference string only.
- Logs never contain keys, and never contain manuscript text by default.

## 7. Testing strategy

| Layer | Tool | What is actually tested |
| --- | --- | --- |
| Domain, repositories, migrations, anchoring, AI apply | `cargo test` | Invariants and data-safety paths against real in-memory SQLite. |
| Stores, services, editor helpers, text utilities | Vitest | Logic that can fail without a window. |
| Workspace flows | Playwright (where practical) | Layout states and keyboard workflow. |

Mocks appear in tests only. No production code path returns fabricated data.
