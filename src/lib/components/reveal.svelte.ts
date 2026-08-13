/**
 * Hover-reveal state for a piece of chrome that focus mode hides (RFC 0082).
 *
 * Five edges of the app use the same rule, so the rule lives here and each zone
 * keeps its own geometry: the reveal is immediate, the collapse waits, and a
 * pin overrides both until the reader unpins it.
 *
 * Owning the timer here is the point — a collapse scheduled just before the
 * zone unmounts must not fire into a dead component, which `reset()` handles.
 */

/**
 * How long a zone stays open after the pointer leaves. Long enough to survive
 * overshooting a button by a few pixels on the way to it, short enough that
 * chrome you have finished with does not linger.
 */
const CLOSE_DELAY_MS = 300;

export class RevealZone {
  #hovered = $state(false);
  #pinned = $state(false);
  #closeTimer: ReturnType<typeof setTimeout> | undefined;

  /** Whether the zone's chrome should currently be shown. */
  get revealed(): boolean {
    return this.#pinned || this.#hovered;
  }

  /** Whether the reader has pinned this zone open. */
  get pinned(): boolean {
    return this.#pinned;
  }

  /** Pointer entered the strip or the revealed chrome: show it at once. */
  enter = () => {
    clearTimeout(this.#closeTimer);
    this.#closeTimer = undefined;
    this.#hovered = true;
  };

  /** Pointer left: collapse after the grace period, unless it comes back. */
  leave = () => {
    clearTimeout(this.#closeTimer);
    this.#closeTimer = setTimeout(() => {
      this.#hovered = false;
      this.#closeTimer = undefined;
    }, CLOSE_DELAY_MS);
  };

  /**
   * Clicking the handle pins the zone open, or lets a pinned one go. Unpinning
   * does not force it shut: the pointer is on the handle, which is inside the
   * zone, and slamming it closed under the cursor leaves nothing to re-enter.
   * `leave` collapses it when the pointer actually goes.
   */
  togglePin = () => {
    this.#pinned = !this.#pinned;
  };

  /**
   * Back to hidden and unpinned, with no timer left running. Called when the
   * zone unmounts, and on every entry into focus mode — RFC 0079 R4.1: each
   * entry is a fresh request for the paper alone.
   */
  reset = () => {
    clearTimeout(this.#closeTimer);
    this.#closeTimer = undefined;
    this.#hovered = false;
    this.#pinned = false;
  };
}
