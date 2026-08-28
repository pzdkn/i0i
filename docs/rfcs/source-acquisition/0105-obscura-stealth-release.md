# RFC 0105: Install the Obscura Stealth Release

- Status: Implemented
- Date: 2026-08-28
- Area: Source acquisition / Packaging
- Builds on: RFC 0102

## Summary

Install Obscura's rendering-enabled `-stealth` release artifact and continue to
start it with `--stealth`. Verify both the artifact selection and runtime
command so i0i does not claim full stealth while shipping the plain transport
build.

## Observed Failure

The manager correctly appends `--stealth` when the default Obscura configuration
is loaded. However, `scripts/setup_obscura.sh` downloads release names such as:

```text
obscura-aarch64-macos.tar.gz
```

Obscura's official release matrix distinguishes that plain build from:

```text
obscura-aarch64-macos-stealth.tar.gz
```

The CLI help explains that the runtime flag enables a consistent browser
fingerprint, while TLS impersonation and tracker blocking require a binary
built with the `stealth` feature. Therefore the flag alone does not establish
the requested full stealth mode.

## Decision

### 1. Select rendering-enabled stealth archives

For every supported OS/architecture pair, append `-stealth` before the archive
extension. Keep the rendering build because i0i uses CDP page inspection; do
not select `-no-render-stealth`.

Explicit installation from an already extracted directory remains supported.
That path trusts the supplied artifact and still performs the existing
execution/signing checks from RFC 0102.

### 2. Keep runtime stealth explicit

Retain `--stealth` in the managed `obscura serve` command. The feature-bearing
artifact supplies the stealth transport, while the runtime flag activates it.
Neither side substitutes for the other.

### 3. Verify the contract without fragile binary introspection

The setup-script test must assert the exact `-stealth` archive URL selected for
each supported platform mapping. The manager test must continue to assert the
runtime `--stealth` argument.

Do not infer compile features from binary size or strings. Obscura does not
currently expose a stable machine-readable build-feature command in i0i's
installed version.

## Scope

### In scope

- Stealth release filenames in the installer.
- Installer tests for supported platform mappings.
- Existing managed runtime `--stealth` behavior.
- Installing the matching local development artifact for live verification.

### Out of scope

- Proxy or residential-IP configuration.
- CAPTCHA solving or challenge bypass.
- Changing browser search providers.
- Modifying Obscura itself.
- Shipping credentials or proxy subscriptions.

## Acceptance Criteria

- [x] Automatic setup selects the rendering-enabled `-stealth` archive for
  macOS and Linux on ARM64 and x86_64.
- [x] Automatic setup never selects a plain or `-no-render` artifact.
- [x] Explicit-directory installation retains signing and execution checks.
- [x] The managed server command includes `--stealth` when stealth is enabled.
- [x] Setup and manager tests pass.
- [x] The locally installed Obscura comes from the matching `-stealth` release
  and starts successfully through i0i's managed command.

## Verification Plan

1. Extend the setup fixture so OS and architecture commands can be controlled.
2. Add failing mapping assertions for the four supported targets.
3. Change only the archive mapping and keep installation behavior unchanged.
4. Run setup-script and manager tests.
5. Install the current host's `-stealth` artifact, start it with the exact
   managed `serve --host 127.0.0.1 --port … --stealth` arguments, and verify its
   CDP health endpoint.

## Implementation Approval

Approved by the user on 2026-08-28. The user explicitly requested Obscura
stealth mode and authorized the agent to write and auto-approve RFCs while they
are AFK, so implementation may proceed without a separate approval round.

## Implementation Notes

Implemented on 2026-08-28. The installer test exercises all four supported
OS/architecture mappings and asserts the exact rendering-enabled stealth
archive URL. Obscura 0.2.1 was installed from the macOS ARM64 stealth artifact,
the managed command retained `--stealth`, and its CDP endpoint started
successfully. Setup, manager, and full Rust tests passed.
