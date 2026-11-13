// A bot's file in the page: its own origin (`botfile:`, no `allow-same-origin`), so it reaches
// neither the page nor the page's calls to the service; the shell's policy keeps it offline.
export const SANDBOX = "allow-scripts";

export const fileUrl = (bot: string, path: string) =>
  `botfile://localhost/${bot}/${path.split("/").map(encodeURIComponent).join("/")}`;
