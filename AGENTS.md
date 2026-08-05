# Agent Instructions

## RFC-First Research Workflow

* For a new analysis, experiment, diagnostic, baseline, comparison, or other
  research feature, discuss and write or revise the RFC first.
* Do not implement code, configuration, tests, plots, or runner wiring for that
  feature until the user explicitly approves moving from RFC design to
  implementation.
* During RFC discussion, keep deliverables to documentation edits unless the
  user separately and explicitly requests code.

## Milestones and RFCs

* Use milestone documents for broad outcomes that require multiple features.
* Each milestone document must include an ordered feature list whose items map
  to focused RFCs.
* Keep each RFC narrow enough to discuss, approve, implement, and verify as one
  coherent feature.
* Treat approval as RFC-specific. Approval of a milestone or one RFC does not
  approve implementation of the other RFCs in that milestone.

## Repository Changelog

* After an RFC feature is implemented and verified, update the `Changelog`
  section of the repository-wide `README.md`.
* Record completed features as checked items using this format:
  `- [x] RFC 0003 - Robot can now eat bananas.`
* Do not mark an RFC complete in the milestone or changelog until its acceptance
  criteria and tests have passed.

## Implementation Principles

### Prefer Simplicity

* Prioritize readability and understandability over cleverness.
* Keep implementations lean and focused.
* Prefer the simplest solution that satisfies the requirements.
* Avoid unnecessary abstractions, patterns, and architectural layers.

### Avoid Speculative Defensiveness

* Trust established internal contracts and invariants that are covered by tests.
* Validate user input, external data, and boundaries where failure could silently
  corrupt results; do not guard against hypothetical internal states already
  prevented by the design.
* Avoid duplicate checks, fallback paths, and compatibility branches that do not
  address an observed failure mode or a concrete requirement.
* Prefer a direct code path and a natural, clear failure over extra defensive
  control flow. Every guard should justify its maintenance cost and added lines.
* When a final validation already protects correctness, do not add intermediate
  validations solely to produce a more specific error message.

### Introduce Abstractions Sparingly

* Only introduce an abstraction when it makes the code easier to understand,
  maintain, or extend.
* Do not create wrappers, helper functions, classes, or modules that merely
  forward calls without adding meaningful value.
* Prefer concrete code over premature generalization.

### Keep Related Code Close

* Organize code so related logic lives together.
* Avoid scattering behavior across many files or layers when a simpler structure
  would be easier to follow.
* Optimize for local reasoning: readers should not need to jump through many
  files to understand a feature.

### Minimize Unnecessary Changes

* Keep diffs as small as reasonably possible.
* Avoid large refactors unless they are required for correctness or
  substantially improve clarity.
* Do not restructure unrelated code while implementing a feature.
* A slightly larger diff is acceptable when it significantly improves
  readability or simplicity.

### Code Quality

* Add type annotations for all new code.
* Prefer self-explanatory code over excessive comments.

### Code Documentation

* Give modules and main classes enough context to understand the domain problem,
  their responsibilities, their boundaries, and their public contracts without
  reconstructing them from the implementation. Where relevant, document
  inputs, outputs, side effects, units, ordering, timing, and state changes.
* Use concise Google-style docstrings for public interfaces, documenting
  meaningful parameters, returns, and real caller-visible errors. Give all
  other functions and methods at least a clear one-line docstring.
* Describe named constants and configuration values where their purpose, units,
  ordering, or effect on behavior is not evident from the name alone.
* In functions or methods with distinct stages, use short phase comments when
  they materially clarify the sequence, lifecycle, or state transitions. Add
  other inline comments only for non-obvious mechanics, invariants, or design
  reasons; do not narrate straightforward code.
* Treat documentation as part of feature completion: keep it accurate as code
  changes and include the documentation pass before acceptance verification.

### Testing

* Implement or update tests for the requested behavior.
* Keep tests focused, readable, and close to the behavior being validated.
* Avoid over-engineered test infrastructure.

### Decision Rule

When choosing between alternatives, prefer the option that is easier for a new
engineer to read and understand, even if it is slightly less abstract or
slightly more verbose.
