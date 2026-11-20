// Where the pane beside the conversation sits, for a window `viewport` px wide.
// Beside the conversation it never takes more than leaves it MIN_CHAT; if even MIN_PANE does not
// fit, it floats over the conversation instead.
export const MIN_PANE = 240;
export const MIN_CHAT = 340;
// The sidebar's `--side-w` (tokens.css). It is narrower only in windows where the pane floats anyway.
export const SIDEBAR = 280;

export function placePane(want: number, viewport: number): { width: number; overlay: boolean } {
  const room = viewport - SIDEBAR - MIN_CHAT;
  if (room < MIN_PANE) return { width: Math.min(Math.max(want, MIN_PANE), viewport * 0.9), overlay: true };
  return { width: Math.min(Math.max(want, MIN_PANE), room), overlay: false };
}
