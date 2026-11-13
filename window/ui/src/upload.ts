import { request } from "./service.ts";

/** Bytes per part: as base64 they stay well under the service's 1 MiB line. */
const PART = 512 * 1024;

/** Sends the file to the bot's `attachments/` folder and gives its path there. */
export async function upload(bot: string, file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let id: string | undefined;
  for (let at = 0; ; at += PART) {
    const part = bytes.subarray(at, at + PART);
    let binary = "";
    for (let i = 0; i < part.length; i += 0x8000) binary += String.fromCharCode(...part.subarray(i, i + 0x8000));
    const last = at + PART >= bytes.length;
    const fields = { bot, data: btoa(binary), last, ...(id ? { upload: id } : { name: file.name }) };
    const reply = await request<{ upload?: string; path?: string }>("attach", fields);
    if (last) return reply.path!;
    id = reply.upload;
  }
}
