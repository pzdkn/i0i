# RFC 0125: VS Code Platform Feasibility

Status: Draft
Date: 2026-09-03
Product: i0i
Decision scope: whether to validate Code - OSS as the future i0i desktop shell

## Summary

Evaluate a thin Code - OSS distribution with a built-in i0i extension before
considering a deeply modified VS Code fork.

The recommended architecture keeps i0i product behavior behind an i0i-owned
interface and treats the host shell as an adapter. Most i0i surfaces fit the
supported VS Code extension interface. Only proven product requirements that
cannot be implemented through that interface should become small, isolated
workbench patches.

This RFC approves no implementation. A separate approval is required for the
feasibility spike and for every later migration RFC.

## Context

i0i is currently a macOS-first Tauri application with:

- a Svelte 5 frontend;
- typed frontend-to-Tauri bridge functions;
- a Rust backend containing the library, reader, discovery, chat, and research
  behavior;
- SQLite-backed local persistence; and
- an IDE-like shell with an activity rail, explorer, editor tabs, inspector,
  and status bar.

The existing shape resembles the VS Code workbench visually, but its runtime
contract is different:

```text
Current
Svelte UI -> Tauri bridge -> Rust commands/services -> SQLite

Candidate
VS Code workbench -> i0i extension -> i0i core process -> SQLite
```

Moving to Code - OSS is therefore a host-platform migration, not a theme or CSS
change.

## Goals

- Determine whether Code - OSS materially improves i0i's shell and interaction
  quality without making upstream maintenance the main engineering activity.
- Test the supported extension interface before changing VS Code workbench
  internals.
- Identify a narrow seam between i0i product behavior and either desktop host.
- Produce evidence for a go, no-go, or extension-only decision.

## Non-Goals

- No production migration.
- No rewrite of the Rust backend in TypeScript.
- No port of every existing i0i screen.
- No VS Code Marketplace integration.
- No deep workbench patches.
- No promise of Windows, Linux, remote, or browser support.

## Options

### Option A: Keep Tauri and redesign the current shell

Continue using the existing Svelte and Rust architecture and improve the visual
system, layout, navigation, accessibility, and interaction details directly.

This has the smallest architectural cost and preserves all current behavior. It
does not inherit the maturity of the VS Code workbench.

### Option B: Ship i0i as a normal VS Code extension

Implement i0i with public extension contributions such as view containers,
tree views, webview views, custom editors, commands, and status items.

This provides the lowest-cost test of the VS Code interaction model and avoids
maintaining a host fork. It cannot substantially modify or remove built-in
workbench UI, and it presents i0i as an addition to VS Code rather than as a
standalone product.

### Option C: Thin Code - OSS distribution with a built-in i0i extension

Rebrand and package Code - OSS, install the i0i extension as a built-in
extension, and set product defaults. Keep core patches at zero initially. Add a
workbench patch only when a validated i0i requirement cannot cross the public
extension seam.

This gives i0i a standalone product while concentrating most product changes in
one extension module. It also creates ongoing responsibilities for upstream
merges, Electron security releases, signing, notarization, updates, extension
distribution, and license notices.

### Option D: Deep VS Code fork

Build i0i features directly in `src/vs/workbench` and freely reshape the host.

This provides the most control and the least architectural restraint. It also
creates the largest permanent patch set, makes upstream updates expensive, and
couples i0i product development to VS Code internals.

## Recommendation

Do not begin with Option D.

If the goal is a VS Code-shaped product, validate Option C by first building the
same i0i extension used by Option B. The built-in extension should own the i0i
UI and communicate with an i0i-owned backend process. Code - OSS should remain
an adapter around that product module.

The deletion test is decisive: deleting the Code - OSS host should not delete
i0i's domain behavior or persistence model. It should reveal another possible
host adapter, such as the current Tauri shell.

The longer-term goal of agent-authored code and remote experiments does not by
itself require i0i to become an IDE. Code can remain a reviewable research
artifact, while execution and experiment state live behind the i0i core
interface. A purpose-built i0i desktop client can embed focused code, diff,
terminal, log, metric, and artifact views without reproducing the complete VS
Code workbench.

The default product direction should therefore remain a purpose-built i0i UI
plus a first-class CLI. A VS Code extension should be considered a companion
client for researchers who already work in VS Code. The Code - OSS distribution
becomes the preferred primary client only if the feasibility spike shows that
users need continuous, general-purpose manual coding inside i0i rather than
review and intervention around agent-authored code.

## Proposed Product Seam

The i0i core interface should expose coarse product operations and event
streams, not Tauri-specific commands or VS Code-specific objects. The exact
transport is intentionally deferred to the spike.

```text
                       +----------------------+
Tauri/Svelte adapter ->|                      |
                       |  i0i core interface  |-> domain behavior -> SQLite
VS Code extension ---->|                      |
                       +----------------------+
```

The existing Rust services are the starting implementation. Tauri commands are
currently the host adapter and should not become the new shared interface by
default.

The same interface should support multiple clients:

```text
Purpose-built desktop UI --+
CLI -----------------------+--> i0i core --> local/remote execution adapters
VS Code extension ---------+                   |--> SSH
Agent runtime -------------+                   |--> Slurm
                                                |--> cloud batch/Kubernetes
```

Remote execution belongs behind explicit adapters with durable run identities,
state transitions, logs, cancellation, reconnect behavior, and artifact
provenance. It should not depend on whichever interactive UI happens to be
open.

## Audience Fit

There is no single interface preferred by all researchers:

- computational researchers and research software engineers often value IDE,
  terminal, Git, notebook, and remote-development integration;
- data scientists often value interactive code, variables, plots, tables, and
  notebooks together;
- literature-heavy, experimental, clinical, social-science, and humanities
  workflows generally benefit more from a purpose-built visual interface than
  from developer tooling; and
- automation and reproducibility benefit from a CLI regardless of the primary
  interactive client.

A terminal-only product would be fast, scriptable, remote-friendly, and easy
for agents to drive. It would be a poor sole interface for PDFs, annotations,
evidence relationships, experiment comparison, plots, approval, recovery, and
discoverability. The CLI should be a complete control interface, not the only
human interface.

For broad researcher appeal, the expected ordering is:

1. a focused i0i UI;
2. a cohesive research-specific Code - OSS distribution;
3. a VS Code extension;
4. a terminal-only application.

For computational researchers already living in VS Code, the extension or
distribution may rank first. This is an audience segmentation claim to validate
in user research, not a universal preference.

## Target Interaction Model

The primary UI should organize the research lifecycle rather than organize
files:

1. **Understand:** papers, evidence, notes, claims, and project state.
2. **Plan:** proposed code or experiment changes, resources, cost, and expected
   outputs.
3. **Review:** diffs, commands, environment, data access, and permissions.
4. **Run:** local or remote execution with durable identity and explicit
   cancellation.
5. **Observe:** state, logs, metrics, checkpoints, failures, and resource use.
6. **Compare:** runs, hypotheses, outputs, provenance, and conclusions.
7. **Intervene:** edit a parameter, open code in an IDE, retry from a checkpoint,
   or stop the run.

Code editing and a terminal are contextual utilities inside this flow. They
should not define the product's top-level information architecture unless i0i
is intentionally repositioned as an IDE for computational research.

## Surface Mapping

| Current i0i surface | Candidate VS Code surface | Expected fit |
| --- | --- | --- |
| Activity rail | View container | Native |
| Vault/project explorer | Tree view | Native |
| Workspace tabs | Editor groups | Native |
| PDF and HTML reader | Custom editor or webview panel | Good, with migration work |
| Research workspace | Webview panel | Good, but largely custom UI |
| Inspector | Secondary-sidebar view or webview view | Good |
| Commands and shortcuts | Commands and keybindings | Native |
| Status | Status bar item | Native |
| Settings | Contributed settings plus focused custom UI | Mixed |
| Rust/Tauri commands | Local process protocol | Requires a new adapter |
| Scheduled/background runs | Durable local process | Requires lifecycle design |

## Public Extension Boundary

An extension can:

- contribute commands, keybindings, settings, menus, themes, icons, and status
  bar items;
- add an Activity Bar or Panel view container;
- render native tree views for vaults, projects, papers, and run history;
- render arbitrary owned HTML, CSS, and JavaScript inside a webview panel or
  webview view;
- provide custom editors for paper-like resources and participate in editor
  tabs, splits, save, undo, and redo where the chosen custom-editor interface
  supports them;
- open documents, terminals, output channels, notifications, progress UI,
  Quick Picks, and native file pickers;
- read and write local workspace files and extension-owned storage;
- use the desktop Node extension host to call local processes and tools;
- contribute language, notebook, testing, source-control, debugging, chat, and
  authentication behavior where useful; and
- observe documented editor, workspace, window, and configuration events.

An extension cannot use the stable public interface to:

- access or rewrite the VS Code workbench DOM;
- inject a global stylesheet or arbitrary HTML into built-in UI;
- change the structure or pixel-level appearance of the title bar, Activity
  Bar, Side Bar, editor tabs, Command Palette, Settings editor, notifications,
  or status bar;
- remove, replace, or deeply reorder built-in workbench modules;
- add arbitrary controls at locations without a contribution point;
- intercept every internal action or lifecycle transition;
- call Electron renderer or main-process interfaces directly;
- assume that undocumented commands, context keys, CSS selectors, or internal
  modules are stable; or
- ship a normal stable Marketplace extension that depends on proposed
  extension interfaces.

Within its own webview, the extension controls the complete application UI but
communicates with the extension host by message passing. The surrounding frame,
tab, toolbar placement, focus model, and workbench layout still belong to VS
Code.

For a private Code - OSS distribution, proposed interfaces can be enabled and
new contribution points can be added. Both choices create an upstream tracking
obligation and should be treated as host patches, not ordinary extension code.

## Principal Risks

### Upstream maintenance

VS Code changes continuously. Every direct workbench modification increases
merge cost and the chance that security or Electron updates are delayed.

Mitigation: enforce a patch budget and require an RFC for each core patch.

### Extension ecosystem

Code - OSS is MIT licensed, but a downstream product cannot assume access to
Microsoft's Visual Studio Marketplace or that Microsoft-published extensions
are licensed for the fork. The product needs an explicit extension strategy,
such as no marketplace, Open VSX, or an i0i-controlled catalog.

### Backend migration

The current backend is embedded in a Tauri application contract. A Code - OSS
host needs a standalone process, transport, startup and shutdown behavior,
crash recovery, logging, version negotiation, and packaging for native assets.

### UI migration illusion

Webviews can reuse web technology, but existing Svelte screens do not become
native VS Code views automatically. Native tree views and commands require new
extension implementations; rich readers can reuse more UI inside webviews.

### Product mismatch

VS Code optimizes for files, folders, source editing, and developer workflows.
i0i optimizes for papers, evidence, projects, and long-running research. The
workbench is valuable only if its interaction model improves those workflows
instead of forcing them into file-editor metaphors.

## Complexity Estimate

These are planning ranges for one engineer familiar with the current i0i
codebase. They are not delivery commitments.

| Outcome | Rough effort | Confidence |
| --- | --- | --- |
| Extension-only interaction prototype | 1-2 weeks | Medium |
| Branded Code - OSS development build with the prototype built in | 2-4 additional weeks | Medium-low |
| Migrate one vertical slice through a standalone Rust process | 3-6 additional weeks | Low |
| Reach broad functional parity with current i0i | 3-6 months | Low |
| Reach polished, signed, updateable product parity | 6-12 months | Low |

Ongoing cost depends primarily on the number and depth of workbench patches. A
near-zero-patch distribution can be maintained periodically; a deep fork is a
continuing product-platform commitment and likely needs dedicated capacity.

## Feasibility Spike

If explicitly approved, a later implementation RFC should deliver only one
vertical slice:

1. Run an i0i extension in ordinary VS Code without a fork.
2. Show a native i0i activity item and vault tree.
3. Open one paper in a custom editor or webview.
4. Read real data through a minimal standalone Rust process derived from the
   existing backend.
5. Record startup time, memory use, UI limitations, packaging obstacles, and
   the exact workbench changes that appear necessary.
6. Repeat the extension unchanged in a locally branded Code - OSS build.

## Acceptance Criteria for the Spike

- One real vault can be opened and one real paper can be read.
- No production data is migrated or duplicated silently.
- The same i0i extension runs in stock VS Code and Code - OSS.
- All proposed direct workbench patches are enumerated and justified.
- The extension-store, signing, notarization, update, and native-binary plans
  are documented.
- A measured go, no-go, or extension-only recommendation is produced.

## Decision Gates

Proceed from the spike to migration only if:

- the VS Code shell demonstrably improves the target workflows;
- the required core patch set is small and bounded;
- the Rust backend can remain the domain implementation behind a narrow host
  interface;
- memory and startup costs are acceptable for the product; and
- the distribution and extension strategy is legally and operationally clear.

## Open Questions

- Is i0i intended to be a research product for developers, or a general
  research product that merely benefits from IDE-like interactions?
- Which exact current UI failures are solved by VS Code rather than by a focused
  redesign of the Tauri shell?
- Must i0i support third-party VS Code extensions?
- Is macOS-only still acceptable after adopting a cross-platform host?
- What maximum workbench patch count and upstream lag are acceptable?
