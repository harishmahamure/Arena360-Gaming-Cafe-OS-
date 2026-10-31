// When the marketing hero's background video may play. It is muted, loops, and never plays for a
// visitor who asked for less motion, while it is scrolled out of view, after the visitor paused
// it, or after it failed to load (the still cover, then solid `bg`, show instead).

/**
 * @param {{ reducedMotion: boolean, visible: boolean, paused: boolean, failed: boolean }} state
 */
export function shouldPlay({ reducedMotion, visible, paused, failed }) {
  return !reducedMotion && visible && !paused && !failed;
}
