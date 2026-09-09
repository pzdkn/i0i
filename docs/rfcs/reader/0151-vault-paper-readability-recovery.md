# RFC 0151: Vault Paper Readability Recovery

- Status: Implemented
- Date: 2026-09-09
- Scope: papers already retained in a Vault that cannot be opened in Reader

## Problem

Several papers in the `circuits` Vault cannot currently be read or opened. A
retained paper may have metadata while its active source is missing, blocked,
failed, stale, HTML-only, or locally unavailable. The current UI collapses these
different states into an unsuccessful open action and gives the researcher no
clear recovery path.

## Diagnostic Contract

Classify every affected Vault paper using persisted facts before changing code:

| State | Evidence | Expected recovery |
| --- | --- | --- |
| Cached PDF | Existing readable local PDF | Open immediately in Reader |
| Remote PDF | Actionable PDF URL, no cache | Acquire, cache, then open |
| Blocked PDF | Direct fetch blocked | Retry through managed browser acquisition |
| Landing page | No usable PDF but actionable web page | Open readable HTML in Reader |
| Stale source | Local path is missing or source status disagrees with disk | Reacquire from stored URLs |
| Metadata only | No actionable source URL or local file | Explain that no source is known and offer source discovery |
| Invalid document | Downloaded payload is not a valid PDF or readable HTML | Reject the payload and continue to the next source candidate |

The diagnosis must report, per paper: title, paper id, active source, all known
source URLs, persisted source status/error, local-file existence, extraction
status, and the Reader result. It must not infer availability solely from an
OpenAlex `pdf_url` field.

## Confirmed Reproduction

The current `circuits` Vault contains 19 papers:

- 12 have cached local PDFs. Every file exists, has non-zero size, starts with
  `%PDF-`, and has a ready extraction. These papers do not reproduce a missing
  Pdfium or corrupt-cache failure.
- 5 use `metadata_abstract` as their active source even though a
  `remote_available` HTML source also exists: *Sparse Feature Circuits*,
  *Towards Monosemanticity*, *Towards Verifiable Transformers*, *Tracing
  Attention Computation Through Feature Interactions*, and *Sparse
  Autoencoders Enable Scalable and Reliable Circuit Identification*.
- 2 have only a `remote_available` HTML source and no active source: *Certified
  Interventional Fidelity* and *MIB*.

The seven HTML-backed records expose two related bugs in the saved-paper path:

1. `ReaderService::get_saved_reader_document()` accepts a
   `remote_available` HTML source as if it were already cached and constructs an
   HTML Reader document.
2. `get_reader_html()` then attempts to read a local snapshot that has never
   been acquired, so opening fails at the byte-loading boundary.

For the five metadata-first records, caching a richer source does not fix source
selection because the store updates `active_source_id` with `coalesce`. Once a
metadata abstract is active, the newly cached HTML source is not promoted.

## Decision

Opening a retained Vault paper resolves its known sources without trusting the
current active source or database insertion order:

1. Rank all known sources: valid cached PDF, cached HTML, remote PDF, remote
   HTML, then metadata abstract.
2. Use a valid cached local document immediately.
3. Acquire selected remote HTML before constructing its Reader document.
4. Use managed browser acquisition when direct HTML fetching is blocked.
5. If no PDF is recoverable, use a readable HTML article or landing page inside
   i0i.
6. Persist a newly cached richer document as the active source, replacing a
   metadata-only active source.
7. If i0i cannot recover readable content, show one concise reason and an
   `Open website` action for the best known landing URL.

Reader opening and background research must use the same resolver and persist
the same source outcome. A paper must not be counted as readable merely because
metadata or a URL exists.

## Feedback Loop

Create a local, deterministic diagnostic command or focused integration test
that accepts a Vault id and returns the classification above without performing
paid model calls. Network recovery probes remain explicit because publisher
responses are unstable; persisted-state classification and local-file checks
must be testable offline.

Use the current `circuits` Vault as the initial real-world reproduction set.
Capture the failing paper ids and exact boundary where each fails before writing
the fix.

## Acceptance

- Each of the seven HTML-backed papers attempts to acquire a durable HTML
  snapshot when opened.
- When HTML acquisition succeeds, Reader serves the cached snapshot and the
  paper promotes it over a metadata-only active source.
- When HTML acquisition fails, Reader retains an actionable `View original`
  route to the known source URL.
- Valid cached documents never wait for network acquisition.
- Blocked direct HTML requests attempt the existing managed browser path once.
- A cached HTML record whose local snapshot is missing is reacquired from its
  stored URL.
- Focused offline tests cover the reproduced source-selection, acquisition, and
  promotion behavior.
- Network-backed checks are explicit and are not part of routine test runs.

## Non-Goals

- Bypassing authentication or publisher paywalls.
- Guaranteeing that every paper has freely available full text.
- Running metadata enrichment or PDF text extraction before the document opens.
- Redesigning the Reader or Vault layout.

## Implementation

- Saved Reader resolution now considers every source for the paper instead of
  trusting only `active_source_id` or insertion order.
- A selected remote HTML source is acquired and stored before Reader returns an
  HTML document.
- HTML acquisition uses direct HTTP first and the configured Obscura browser
  fallback when direct fetching fails.
- Caching a full document replaces a metadata abstract as the paper's active
  source and clears the abstract extraction pointer.
- If HTML acquisition still fails, Reader keeps the HTML source URL available
  so the researcher can open the original page.

## Verification

- `cargo test --no-default-features --lib`
- Result: 644 passed, 0 failed, 11 explicitly ignored live/manual tests.
- Focused regressions cover browser-backed HTML acquisition, HTML selection over
  an active metadata abstract, and active-source promotion after caching.
