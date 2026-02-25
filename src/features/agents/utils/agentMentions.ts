import type { AgentMention } from "../../../types";

export type AgentMentionBinding = {
  slug: string;
  agentName: string;
};

const MENTION_NAME_CHAR = /[A-Za-z0-9_-]/;

export function collectAgentMentionNames(text: string): Set<string> {
  const names = new Set<string>();
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] !== "@") {
      continue;
    }
    const prev = index > 0 ? text[index - 1] : "";
    if (prev && MENTION_NAME_CHAR.test(prev)) {
      continue;
    }
    let end = index + 1;
    while (end < text.length && MENTION_NAME_CHAR.test(text[end])) {
      end += 1;
    }
    if (end === index + 1) {
      continue;
    }
    names.add(text.slice(index + 1, end).toLowerCase());
    index = end - 1;
  }
  return names;
}

export function resolveBoundAgentMentions(
  text: string,
  bindings: AgentMentionBinding[],
): AgentMention[] {
  if (!text || bindings.length === 0) {
    return [];
  }
  const names = collectAgentMentionNames(text);
  if (names.size === 0) {
    return [];
  }

  const seenNames = new Set<string>();
  const mentions: AgentMention[] = [];
  for (const binding of bindings) {
    if (!names.has(binding.slug)) {
      continue;
    }
    if (seenNames.has(binding.agentName)) {
      continue;
    }
    seenNames.add(binding.agentName);
    mentions.push({ name: binding.agentName });
  }
  return mentions;
}
