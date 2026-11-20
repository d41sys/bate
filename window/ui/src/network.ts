/** The hosts typed one per line (or separated by commas or spaces), without the empty ones. */
export function hostLines(text: string): string[] {
  return text.split(/[\s,]+/).filter(Boolean);
}

export const hostsHint = "One host per line: example.com, *.example.com (its subdomains), 10.0.0.5:8080. Without a port, 80 and 443.";
