/** A note's size as the panel shows it: bytes, then KiB. */
export function size(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KiB`;
}
