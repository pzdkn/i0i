# RFC 0102: Reliable macOS Obscura Resource Launch

- Status: Draft
- Date: 2026-08-27
- Area: Source acquisition / Packaging
- Builds on: RFC 0041, RFC 0100

## Summary

Make Obscura executable on macOS by selecting the verified source resource in
development, signing resources appropriately for their build context, and
verifying the actual selected executable before i0i starts. A taskgated
failure must be identified as such instead of appearing to be a generic
browser startup failure.

## Observed Failure

On macOS 26.5.1 on Apple silicon, eager browser startup fails immediately:

```text
obscura: starting executable=.../resources/obscura/obscura port=51772 timeout_ms=10000
browser_process_unavailable: Obscura exited before becoming healthy:
status=signal: 9 (SIGKILL); stderr=<empty>
```

The failure is deterministic outside Tauri as well. Both `obscura --version`
and `obscura --help` exit with status 137 before printing output. The macOS
diagnostic report identifies the termination precisely:

```text
SIGKILL (Code Signature Invalid)
namespace=CODESIGNING
indicator=Taskgated Invalid Signature
```

This is not a port-selection, startup-timeout, architecture, or missing-worker
failure:

- the host and both Obscura executables are ARM64;
- the launcher targets macOS 11 or newer and loads only system libraries;
- `obscura-worker --help` exits successfully;
- the launcher is killed even for commands that do not start a browser;
- `codesign --verify --deep --strict` reports the file as valid on disk, so
  that static check alone does not reproduce taskgated's execution decision.

As a controlled diagnostic, an unchanged temporary copy of the launcher was
ad-hoc re-signed with:

```text
codesign --force --sign - --timestamp=none obscura
```

That copy printed `obscura 0.1.9`, started Chrome 145, and returned a healthy
`/json/version` response containing a CDP WebSocket URL. Re-signing therefore
changes the failing condition directly and is sufficient to start the managed
browser on the affected machine.

A follow-up reproduction exposed a second boundary. The source resource at
`src-tauri/resources/obscura/obscura` executes successfully, while the byte-for-
byte identical copy at `src-tauri/target/debug/resources/obscura/obscura` is
killed by taskgated. Both files have the same SHA-256 hash, embedded signature,
architecture, mode, and provenance attribute. The runtime copy still reports
the release artifact's `adhoc,linker-signed` flags after `tauri dev` starts.

Therefore a one-off signature applied to the runtime path before starting the
development build is not a durable workaround: Tauri can replace that file
during resource staging. Verification must cover the executable actually
selected after staging.

The direct source path is a validated development bypass. Starting
`src-tauri/resources/obscura/obscura serve` on the affected machine returns a
healthy Chrome 145 `/json/version` response and CDP WebSocket URL. The running
i0i process has `src-tauri` as its working directory, and the existing
`[obscura].path` configuration contract can select this source executable.

## Decision

### 1. Prefer the verified source resource in development

An explicit `[obscura].path` remains authoritative. Without an override, debug
builds should prefer the repository source resource resolved from the current
working directory before Tauri's staged `target/debug/resources` copy.

Release builds continue to resolve the packaged resource directory. Do not
make a source-tree path a production dependency.

This keeps the development path simple and uses the exact executable verified
by the setup script. It also avoids relying on a staged file identity that
taskgated rejects even when its bytes and embedded signature match the working
source.

### 2. Sign development resources after installation on macOS

After `scripts/setup_obscura.sh` copies both Obscura executables into
`src-tauri/resources/obscura`, ad-hoc sign those installed copies on macOS.
Sign the launcher and worker explicitly rather than relying on the signature
embedded in a downloaded archive.

This applies only to local development resources. It does not replace the
distribution identity used for a release build.

### 3. Treat release signing as a separate build context

A distributable macOS application must sign its embedded executables through
the normal application signing and notarization pipeline. The build must not
ship the local development ad-hoc signature as its final trust boundary.

Keep the setup script's local signing step independent from release credentials
so contributors do not need a Developer ID certificate to run i0i locally.

### 4. Verify execution, not only signature metadata

At the end of macOS resource setup, run the installed launcher with
`--version`. Installation fails if the command does not exit successfully.

The development verification must also execute the path selected by
`ObscuraConfig` after Tauri stages resources. Before this RFC is implemented,
that is `src-tauri/target/debug/resources/obscura/obscura`; afterward it should
be the verified repository source resource. A successful check of a path the
manager does not select does not establish that managed startup works.

Do not use `codesign --verify` as the only acceptance check. In the observed
failure, it returned success while taskgated killed the same file at execution.

The smoke check should also print the installed version so logs identify which
Obscura artifact was verified.

### 5. Preserve the runtime boundary

`ObscuraManager` should continue to report early exit status and stderr as
specified by RFC 0100. The manager cannot repair an invalid executable after
spawn and must not mutate bundled resources at runtime.

For macOS `SIGKILL` with empty stderr, documentation may direct developers to
inspect `~/Library/Logs/DiagnosticReports/obscura-*.ips`. Runtime code should
not parse private operating-system crash reports.

## Scope

### In scope

- Ad-hoc signing of locally installed macOS Obscura resources.
- Debug-time selection of the verified source resource.
- A post-install `obscura --version` execution smoke check.
- Documentation of the distinct development and release signing contexts.
- A focused setup-script test or macOS CI check for the signing commands.

### Out of scope

- Changing Obscura process lifecycle or health checks.
- Increasing the browser startup timeout.
- Disabling macOS Gatekeeper, SIP, or code-signing enforcement.
- Managing Developer ID credentials or notarization secrets in this repository.
- Rebuilding or patching Obscura itself.

## Acceptance Criteria

- [ ] On macOS, `scripts/setup_obscura.sh` signs both installed executables
  after copying them into the resource directory.
- [ ] An explicit `[obscura].path` remains authoritative.
- [ ] Without an explicit path, debug startup prefers the verified repository
  source resource over the rejected staged copy.
- [ ] Release startup continues to use the packaged resource directory.
- [ ] Setup fails if the installed `obscura --version` command is killed or
  otherwise exits unsuccessfully.
- [ ] Setup logs the verified Obscura version.
- [ ] Linux setup behavior is unchanged.
- [ ] A Tauri development build selects the verified source resource and
  reaches a healthy `/json/version` endpoint through `ObscuraManager`.
- [ ] The launcher selected after resource staging passes `--version`.
- [ ] Release documentation states that embedded executables receive the
  distribution signature during the macOS application build.
- [ ] Focused setup checks and existing Obscura manager tests pass.

## Verification Plan

1. Capture the current failing executable as the pre-fix signal:
   `obscura --version` exits 137 and its diagnostic report names
   `Taskgated Invalid Signature`.
2. Add a macOS setup check that inspects the intended signing commands without
   downloading a release artifact.
3. Apply local ad-hoc signing after both resource copies.
4. Run the setup script, then run the installed launcher's `--version` command.
5. Rebuild the Tauri development resources and call `/json/version` through
   the managed Obscura startup path.
6. Run the existing Obscura manager tests and the full Rust library suite.

## Implementation Approval

This RFC records the diagnosis and proposed fix only. No setup script, bundled
binary, runtime code, tests, or release configuration should change until the
user explicitly approves implementation of RFC 0102.
