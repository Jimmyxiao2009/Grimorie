# Grimoire — Rebuild Baseline

_Recorded at the start of the ground-up rebuild._

## 1. State of the repository at audit time

The audit was performed against the working directory `E:\Desktop\项目\Grimorie`.

Findings:

- The directory was **empty**. No files, no subdirectories.
- There was **no git repository** (`git status` reported `fatal: not a git repository`).
- Consequently there was **no git history**, no branches, and no prior commits.
- There was **no previous Grimoire implementation present on disk** to inspect.

This is stated plainly because the rebuild brief anticipated an existing implementation
that could be mined for product ideas. No such implementation was available. Nothing was
deleted, overwritten, or discarded to reach this state — the directory was already empty.

**Implication:** there is no legacy code to preserve, no migration path to honour, and no
user data at risk. The rebuild is a genuine greenfield build, and the "do not preserve old
architecture merely because it exists" rule is satisfied vacuously.

## 2. Prior product ideas carried forward

Because no prior codebase was available, the product ideas carried into this build come
from the rebuild brief itself rather than from archaeology. The ideas judged worth building
around:

| Idea | Why it is worth keeping |
| --- | --- |
| Library → Volume → Chapter → Page hierarchy | Gives long-form work real structure without becoming a general-purpose outliner. |
| The Margin as a first-class region | Separates *writing* from *thinking about writing*. This is the distinguishing feature. |
| AI confined to the Margin | Keeps the manuscript under user control; AI advises, it does not author. |
| Anchored annotations with staleness detection | Prevents the classic failure where a comment silently attaches to the wrong text. |
| Local-first, user-owned data | Manuscripts are private. No cloud, no telemetry, no account. |
| Surface Go as the reference device | Forces genuine responsive information architecture instead of a shrunken desktop UI. |

## 3. Architecture explicitly *not* reused

Nothing is reused, because nothing exists. For the avoidance of doubt, the following are
ruled out by design decision rather than by inheritance:

- Electron or any second browser runtime beyond the Tauri WebView.
- React / Next.js.
- Markdown as the canonical document representation.
- A cloud backend, sync server, account system, or telemetry pipeline.
- CRDTs and real-time collaboration.
- A plugin or scripting runtime.
- A Redux-style global store or a general-purpose event bus.

## 4. Rebuild boundaries

**In scope for this iteration** — the MVP defined in the brief: volumes, chapters, pages, a
Tiptap-based editor, autosave, crash recovery, revision history, anchored margin
annotations, FTS5 search, bookmarks, tags, an OpenAI-compatible AI layer with streaming and
secure credential storage, the AI review → margin annotation workflow, safe suggestion
apply, focus mode, themes, backup/import/export, and a deliberate Surface Go responsive
pass.

**Out of scope for this iteration** — Surface Pen ink capture (the data model reserves room
for it; the capture UI is deferred), PDF/DOCX/HTML export, macOS and Linux packaging
(the architecture stays portable but only Windows is targeted), and any provider beyond
OpenAI-compatible HTTP APIs.

## 5. Verified toolchain

Measured on the build machine at audit time, not assumed:

```
node      v24.19.0
npm       11.17.0
pnpm      11.9.0
rustc     1.97.1 (8bab26f4f 2026-07-14)
cargo     1.97.1 (c980f4866 2026-06-30)
git       2.53.0.windows.2
rust target   x86_64-pc-windows-msvc (installed)
linker        Visual Studio Community 2026 (MSVC, verified by a successful test link)
WebView2      151.0.4129.93 (runtime present)
registries    crates.io and npm both reachable
```

`aarch64-pc-windows-msvc` is **not** installed. Windows ARM64 packaging therefore requires
`rustup target add aarch64-pc-windows-msvc` before it can be produced; the configuration is
written to support it, but an ARM64 binary has not been built or tested here.

## 6. Branch

Work proceeds on `rebuild/grimoire-next`, branched from the initial commit on `main`.
