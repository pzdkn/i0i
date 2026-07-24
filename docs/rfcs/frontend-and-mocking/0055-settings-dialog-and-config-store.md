# RFC 0055: Settings Dialog and Configuration Store

Status: Implemented (v1)
Date: 2026-07-23
Product: i0i
Target: Tauri v2 + Svelte, macOS first
Builds on: RFC 0001 (App Shell), RFC 0037/0043/0044 (discovery), RFC 0049/0050
(metadata autofill), RFC 0051 (source acquisition), RFC 0053 (provider
expansion), RFC 0054 (search relevance engine)

## Summary

Give i0i an in-app **Settings** surface, opened from the SETTINGS button that
already exists (unwired) in the activity rail. Behind it, add a small
**configuration store** in the app data directory so users can set the things
that are today invisible and developer-only — most importantly **API keys** and
**model/cost choices** — without editing a repo-local `.env`.

The decision is:

- A **modal dialog** opened from the existing `ActivityRail` SETTINGS button,
  with tabs down the left. (First reusable modal in the app.)
- A **settings store** persisted in the app data dir, read by the backend.
- A **three-source resolution chain** for secrets and config:
  **user settings → environment variable → `.env`**. The existing `.env` dev
  workflow keeps working untouched; a value set in the UI simply wins.
- v1 ships three tabs: **API Keys**, **Models & cost**, **Search defaults**.
  Other tabs (Acquisition, Data & storage, Appearance, About) are enumerated
  but deferred.
- Secret values are **never returned to the renderer** — the UI sees only
  "configured / not / from where", and writes new values one-way.

Plain English: there's currently no way for anyone who isn't editing the repo to
give i0i an API key or pick a cheaper model. This adds the screen that does that,
and the small backend plumbing to make a saved value actually take effect.

## Problem

1. **Keys are developer-only.** Every API key (OpenRouter, OpenAlex, CORE) and
   the Unpaywall contact email are resolved at startup from an environment
   variable or a repo-local `.env` (`resolve_api_key` does env → `.env`). A
   shipped desktop app has no editable `.env` next to the binary, so a
   non-developer literally cannot configure the app. Discovery, chat, deep
   research, query expansion, and OA PDF resolution all silently no-op or
   degrade without their key, with no UI to fix it.

2. **Consequential config is invisible.** Model choices live in
   `app.conf.json` (`anthropic/claude-sonnet-4.5` for chat, `title_model` for
   titles/expansion) — the single biggest cost lever — and the user can't see
   or change them. The RFC 0054 embedding model downloads ~130 MB on first run
   with no status or control. RFC 0051's acquisition timeouts are noted as
   "overridable later via config". None of it is reachable.

3. **No config-writing path exists.** The app reads config; it has never
   written any. There is no settings store, no settings command, and the
   activity-rail SETTINGS button has no `onclick`.

## Goals

- Wire the existing SETTINGS button to a real, reusable modal.
- Persist user settings in the app data dir and resolve them ahead of env/`.env`.
- Let a user set the three API keys + contact email from the UI and have them
  take effect (immediately where services resolve lazily; restart-flagged
  otherwise).
- Surface model choice per LLM job and the embedding-model status/controls.
- Keep secret values server-side: the renderer never receives a stored key.
- Preserve the current `.env`/env developer workflow with zero changes.

## Non-Goals

- **No OS keychain in v1.** Secrets are stored as plaintext in the app data dir
  (acceptable for a local single-user tool); keyring/stronghold is Future Work.
- No sync/cloud settings, no multi-profile.
- No settings for every internal constant — only knobs that are user-facing and
  consequential (keys, models, cost, downloads, what-leaves-the-machine).
- No redesign of the activity rail or workspace layout.
- Appearance/theming, acquisition tuning, and data/storage tools are scoped as
  later tabs, not v1.

## Placement

- **Entry point:** `ActivityRail.svelte` already renders a `SETTINGS ?` button
  at the bottom with no handler. Wire its `onclick` to open the modal. No new
  rail affordance needed.
- **Shape:** a centered **modal overlay** (`Modal.svelte`, the app's first),
  with a left tab list and a content pane. Chosen over a full mode-view because
  settings are occasional and a modal doesn't disturb the tabs/vault layout.
  (SETTINGS is a rail mode key `?`; a full-view is possible later if settings
  outgrow a dialog — the store/commands below don't change either way.)
- Escape / backdrop-click / a close button dismiss it; unsaved edits prompt or
  save-on-change per field (see UX below).

## Backend Design

### Settings store

A tiny key/value store in the existing SQLite database (the `library_store`
already owns a DB in the app data dir), new table:

```sql
CREATE TABLE IF NOT EXISTS app_settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

Keys are namespaced strings (`secret.openrouter_api_key`,
`model.chat`, `search.default_expand`, …). A JSON file in the app data dir is
the alternative; SQLite is chosen because the DB already exists, gets
transactional writes for free, and avoids a second storage mechanism.

### Resolution chain

Introduce one helper that centralizes the priority order:

```rust
// user settings store  ->  environment variable  ->  .env
fn resolve_secret(store: &SettingsStore, setting_key: &str, env_key: &str) -> Option<String>
```

Refactor the existing per-config resolvers (`ChatConfig::resolve_api_key`,
`OpenAlexConfig::resolve_api_key`, `CoreConfig::resolve_api_key`,
`locations::unpaywall_email`) to call it, passing their setting key and existing
env-var name. The env/`.env` fallback is unchanged, so **the developer workflow
keeps working**; a stored value merely takes precedence.

### Reload semantics

- **Immediate** (no restart): OpenAlex, CORE, and chat/OpenRouter already
  resolve their key lazily per request. Once they read the store first, a saved
  key applies to the next search/message.
- **Restart-flagged**: the embedding reranker (RFC 0054) and the query expander
  construct once at startup. v1 marks changes to their inputs "restart to
  apply". (Optional small follow-up: make the expander resolve lazily like the
  other providers, removing its restart flag.)

### Commands

- `get_settings() -> SettingsView` — returns non-secret settings **plus** a
  per-secret status: `{ configured: bool, source: "user" | "env" | "dotenv" }`.
  **Never returns secret values.**
- `save_setting(key, value)` — upserts one setting; deleting is
  `save_setting(key, "")` → row removed. Secrets are written one-way.
- `clear_setting(key)` — remove a user override (fall back to env/`.env`).
- `test_provider_key(provider)` — issue a minimal live request (a 1-result
  search / a tiny completion) and return ok/err, so the user can verify a key
  without leaving the dialog.

### Security

Secrets live plaintext in the app data DB (single-user local tool). The renderer
never receives them — only status. This bounds the leak surface to the local
disk, which already holds the user's whole library. OS keychain storage is
Future Work behind the same command surface (the frontend wouldn't change).

## Frontend Design

- `Modal.svelte` — reusable overlay (backdrop, focus trap, Escape), first of its
  kind; usable by future dialogs too.
- `SettingsDialog.svelte` — left tab list + content pane, opened via shell state
  toggled by the rail button.
- One component per tab. Each field shows current status from `get_settings` and
  writes via `save_setting`; secret fields render masked with a
  "configured / from .env / not set" pill and a **Test** button.

### Tab inventory (v1)

**API Keys**
- OpenRouter, OpenAlex, CORE keys, plus the contact email (`IOI_EMAIL`).
- Per **key**: masked, write-only input, status pill (set / unset / source),
  **Test**. The **email** is non-sensitive — shown in a plain, prefilled,
  editable field (see Resolved Design Gap 3).
- Each labeled with what it unlocks: OpenRouter → chat + deep research + query
  expansion; OpenAlex → discovery; CORE → the CORE provider; email → better OA
  PDF resolution (Unpaywall).

**Models & cost**
- Model per LLM job: **chat**, **deep-research planner**, **query expansion**
  (from `app.conf.json` today; overridable in the store). Expansion/titles
  defaulting to a cheap model is the main cost lever.
- Master toggles: **query expansion on/off**, **deep research on/off**
  (global defaults; per-workspace toggles still win locally).
- `max_context_chars` for chat (optional/advanced).

**Search defaults**
- Default provider set; global defaults for **Expand my search** and **Only
  results I can open**; default result limit / sort / open-access.
- **Embedding reranker**: an on/off toggle writing the runtime
  `search.reranker_enabled` flag (Resolved Design Gap 1), status
  (ready / downloading / not built in), and a **download/remove model** control
  (the ~130 MB BGE model). This is also the RFC 0054 model-download UX, which is
  currently missing. The download/remove sub-piece (fastembed cache management)
  is the one genuinely complex item here.

### Tab inventory (deferred)

- **PDF acquisition** (RFC 0051): overall deadline / timeouts, Obscura browser
  fallback on/off, metadata autofill manual-vs-automatic and the
  `I0I_METADATA_WEB_LOOKUP` gate.
- **Data & storage**: app-data location readout, **clear caches** (PDF cache,
  RFC 0051 negative cache, embedding model), storage-used.
- **Appearance**: theme / accent.
- **About / diagnostics**: versions, model status, and a plain-language
  **"what leaves your machine"** summary (which features call which external
  services; embeddings + reading are fully local).

## Validation

Backend tests:

```text
settings store: upsert, read, clear round-trips
resolution: user value beats env; env beats .env; missing -> None
resolution: existing .env-only setup still resolves (no regression)
get_settings never includes a secret value, only status + source
save/clear secret updates status source correctly
test_provider_key returns err on a bad key, ok on a good one
```

Frontend checks:

```text
SETTINGS rail button opens the modal; Escape/backdrop close it
secret fields render masked; status pill reflects source
saving a key updates status without exposing the value
Models & cost writes model overrides; Search defaults writes toggles
restart-required note shown for embedding/expander-affecting changes
```

Commands:

```bash
cargo fmt --check
cargo test
cargo check
pnpm check
git diff --check
```

Manual smoke:

```text
1. Open Settings from the rail; set the OpenRouter key; Test → ok.
2. Run a chat/expansion without restart; confirm it works.
3. Set a cheaper expansion model; confirm expansion uses it.
4. Remove a key override; confirm fallback to .env/env.
5. Confirm no secret value ever appears in devtools network payloads.
```

## Implementation Order

1. Add RFC 0055.
2. `app_settings` table + `SettingsStore` (upsert/read/clear) + tests.
3. `resolve_secret` helper; refactor the four resolvers to consult the store
   first; regression-test the `.env` path.
4. Commands: `get_settings` (status-only for secrets), `save_setting`,
   `clear_setting`, `test_provider_key`.
5. `Modal.svelte` + `SettingsDialog.svelte`; wire the rail button.
6. API Keys tab.
7. Models & cost tab (store-backed model overrides; read at resolve time).
8. Search defaults tab + embedding-model status/controls.
9. Validation.

## Risks

- **Secret exposure to the renderer.** Mitigated by never returning values —
  status only — and writing one-way. Reviewer check: no command serializes a
  stored secret.
- **Stale startup-constructed services.** The reranker/expander won't pick up a
  new key until restart in v1; mitigated by an explicit "restart to apply" note
  and by most services already being lazy. Making the expander lazy is a cheap
  follow-up.
- **Plaintext at rest.** Accepted for a local single-user tool; the DB already
  holds the library. Keychain is Future Work behind the same commands.
- **Config drift between `app.conf.json` and the store.** The store overrides;
  `app.conf.json` remains the default/seed. Resolution always reads store-first
  so there is one precedence rule.

## Implementation Notes (2026-07-23)

**Slice 1 — store + resolution chain + commands + Modal + API Keys tab — landed
and validated** (220 backend tests pass, `pnpm check` 0/0).

- `services/settings.rs`: `SettingsStore` (own `settings.sqlite`, fresh
  connection per op like `LibraryStore`), a `SettingSource` enum, and the
  precedence logic in a **store-explicit** `resolve_with(store, setting_key,
  env_key) -> (value, source)` so it unit-tests without touching global state.
  A process-global `OnceLock<SettingsStore>` (handle only) backs
  `resolve_secret` / `resolve_secret_with_source` for the free-function
  resolvers. One precedence function; `resolve_secret` discards the source.
- The four resolvers (`ChatConfig`, `OpenAlex`, `CORE`, Unpaywall email) now go
  through `resolve_secret`, so a key saved in the UI is picked up on the next
  request with no restart (all four resolve lazily — verified). The `.env`/env
  fallback is unchanged; existing resolver tests still pass.
- Commands (`commands/settings.rs`): `get_settings` (secret **status only** —
  `configured` + `source`, never values; prefs filtered `not like 'secret.%'`
  at the SQL level), `save_setting`/`clear_setting` (namespace-guarded), and
  `test_provider_key` (save-then-test: OpenAlex/CORE do a 1-result search,
  OpenRouter a 1-token completion, email checks configured).
- Frontend: reusable `components/Modal.svelte` (first in the app; Escape +
  backdrop close, a11y-clean), `features/settings/SettingsDialog.svelte`
  (left tab list, loads on open), `ApiKeysTab` + `ApiKeyRow` (masked input,
  status pill with source, Save/Test/Clear). The activity-rail gear is wired
  through `AppShell` → `+page.svelte`.

**Slice 2 — Models & cost + Search defaults + refinements — landed and
validated** (220 backend tests pass, `pnpm check` 0/0).

- **Models & cost**: overrides for `model.chat`, `model.planner`,
  `model.expansion` (free-text + a curated `datalist`), read at each
  construction point (`ChatConfig::load`, `SearchManager` per-run, the
  expander). Chat/expansion are restart-flagged (their fields cache at startup —
  the fields carry a "restart to apply" note); the planner reads per-run so it
  applies immediately. `settings::init_global` was moved to the **top** of
  `setup()` so startup config reads see overrides.
- **Search defaults**: a runtime **`search.reranker_enabled`** flag consulted by
  `EmbeddingReranker::semantic_scores` (the Resolved-Gap-1 runtime toggle, not
  the Cargo feature), default on, disabled in the UI when the feature isn't
  built (`get_reranker_status`); plus `search.default_expand` /
  `search.default_only_viewable` / `search.default_result_limit`, which seed new
  Discover workspaces via `applyDiscoverDefaults` (loaded on app mount).
- **Refinements**: first-run gear dot when the OpenRouter key is unresolved;
  `meta.schema_version` row; WAL + `busy_timeout`; minimal Modal focus handling
  (focus first field on open, return focus on close); `preference` /
  `preference_bool` readers.
- Reusable `TextPref` / `TogglePref` controls back the two new tabs.

**Deferred (documented, not v1-blocking):** the embedding model
**download/remove** control (fastembed cache management — the one genuinely
complex sub-piece; status is shown, management deferred); per-feature
empty-state prompts linking to Settings (the gear dot is the primary nudge);
default **provider-set** seeding (expand / only-viewable / result-limit are
seeded); and the later tabs (Acquisition, Data & storage, Appearance, About).

## Resolved Design Gaps (revision 2026-07-23)

Review surfaced holes the first draft glossed. Decisions, so slice 2 is built
against them:

1. **Reranker toggle is a runtime setting, not the Cargo feature.** RFC 0054's
   `embeddings` is compile-time and can't be flipped at runtime. Add a runtime
   `search.reranker_enabled` (default on) that the reranker consults before
   scoring; the Cargo feature governs only whether the model is compiled in at
   all. The Search-defaults toggle writes `search.reranker_enabled` and is shown
   disabled ("not built in") when the feature is off.
2. **Model choice is a free-text field with curated suggestions**, not a live
   model browser. Each model input offers a `datalist` of common OpenRouter
   slugs; power users can type any slug. Fetching OpenRouter `/models` for a
   real picker is Future Work — it adds a network dependency and a browsing UX
   this doesn't need yet.
3. **The contact email is not a secret.** It is stored via the same resolution
   chain but is **non-sensitive**: `get_settings` returns its value and the UI
   shows it in a plain, prefilled, editable field. Only true API-key values are
   withheld from the renderer. (Implemented: `SecretSpec.sensitive`; keys →
   `value: None`, email → `value: Some`.)
4. **"Restart to apply" is a per-field inline note**, shown only on fields whose
   consumer resolves at startup (embedding model, and — see below — model
   overrides if we keep them lazy-unfriendly). No global restart banner in v1;
   hot-reloading those services is Future Work.
5. **First-run discoverability**: features that need a key surface an empty
   state linking to Settings, and the activity-rail gear shows a small dot when
   a required key (OpenRouter) is unresolved. The dialog answers "where"; these
   answer "how does a new user know to open it."
6. **SQLite durability**: the store opens each connection with WAL +
   `busy_timeout(5s)` so quick successive saves don't error with `SQLITE_BUSY`.
   (Implemented.)
7. **Value validation**: `save_setting` trims and treats empty as clear;
   secrets/models are non-empty by construction; the email field uses an
   `email`-typed input. Deeper per-field validation (model-slug plausibility)
   is deferred but noted.
8. **Clearing a secret is not a secure erase.** Deleting the row leaves the
   plaintext in the SQLite WAL/freelist until a `VACUUM`. Documented, not
   solved in v1; OS-keychain storage (Future Work) removes the concern
   entirely. Do not describe "Clear" as "wipes from disk."
9. **Modal focus handling is minimal by design**: focus the first field on
   open and return focus to the opener on close. A full focus trap is Future
   Work — the earlier "focus trap" wording overstated v1.
10. **Settings schema versioning**: a `meta.schema_version` row lets a future
    key rename migrate forward. Not needed for the current flat keys but cheap
    insurance to define now.

Of these, 1–3 change what slice 2's tabs *do* and are settled here before
building them; 4–10 are refinements folded into the existing design.

## Future Work

- OS keychain (Tauri keyring/stronghold) for secrets, same command surface.
- The deferred tabs (Acquisition, Data & storage, Appearance, About).
- Make the query expander resolve its key lazily to drop its restart flag.
- Per-model cost estimates in the Models tab (needs the pricing data the
  `claude-api` reference already tracks for Anthropic models).
- Export/import settings (minus secrets).
