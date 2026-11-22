import type { Question } from "./chat.ts";

/**
 * The command that answers `q`. A typed secret goes in `answer_secret` and nowhere else: the service
 * keeps it in the Keychain and tells the bot only that it was saved. Every other answer, a refusal
 * too, is a plain choice that carries no value (PROTOCOL.md, "Secrets for a site").
 */
export const answerCommand = (bot: string, conversation: string, q: Question, choice: string, secret?: string): [string, Record<string, unknown>] =>
  secret === undefined ? ["answer", { bot, conversation, question: q.question, choice }] : ["answer_secret", { bot, conversation, question: q.question, value: secret }];
