# RFC 0109: Project and Vault Ownership Migration

- Status: Implemented and verified
- Date: 2026-09-01
- Area: Projects / Library persistence / Navigation
- Parent: RFC 0108

## Summary

Introduce Project as the goal-directed owner of exactly one Vault. Migrate each
existing Vault into a same-titled Project without changing the Vault id, path,
or Paper memberships. Replace Vault creation and deletion in the Explorer with
atomic Project lifecycle operations and present Projects as the navigation
boundary.

This RFC does not add Project documents, Research State, or Harness execution.
Their Explorer destinations may be shown only when their delivery RFCs make
them functional.

## Domain and Persistence

```text
Project 1 ─── 1 Vault
Vault   * ─── * Paper
```

`Project` initially contains `id`, `title`, and optional `goal`. `Vault` gains a
required, unique `project_id`. A Project title and Vault path are separate after
creation.

Migration creates one Project for every existing Vault:

- Project id is derived deterministically from the Vault id;
- Project title begins as the existing Vault title;
- Vault id and path are unchanged;
- Vault-Paper memberships and canonical Papers are unchanged; and
- repeated startup migration is idempotent.

The store owns atomic lifecycle operations:

- creating a Project creates its Vault in the same transaction;
- renaming a Project changes only the Project title;
- deleting a Project deletes its Vault and memberships, then removes only
  Papers that have no remaining Vault membership.

Legacy internal Vault lifecycle methods must preserve the same ownership
invariants until their remaining callers are migrated.

## Interaction

The Explorer section is named **Projects**. Each row represents a Project and
shows the Paper count of its owned Vault. Creating, renaming, opening, and
deleting rows operate on Projects. Opening a Project displays its Vault content
for this RFC; the dedicated Research landing view arrives with the Harness and
Research State RFCs.

## Acceptance Criteria

1. Every returned Project has exactly one returned Vault and every returned
   Vault names exactly one Project.
2. Existing Vault ids, paths, and Paper memberships survive migration.
3. Migration is idempotent.
4. Project creation is atomic and creates exactly one Vault.
5. Duplicate Project/Vault identity is rejected without partial records.
6. Renaming a Project does not rename its Vault.
7. Deleting a Project removes its Vault but preserves Papers shared through
   another Project's Vault.
8. The frontend Explorer is Project-labelled and uses Project lifecycle
   commands while Vault paper interactions remain Vault-scoped.
9. Rust tests, frontend type checking, and the production frontend build pass.

## Approval

Approved on 2026-09-01 under the user's instruction to author, approve,
implement, verify, and commit each focused delivery RFC from RFC 0108 without a
separate approval round.

## Verification

Implemented and verified on 2026-09-02.

- `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features`
  passed: 487 tests, 5 ignored live/manual tests.
- `pnpm check` passed with no errors or warnings.
- `pnpm build` completed successfully.
