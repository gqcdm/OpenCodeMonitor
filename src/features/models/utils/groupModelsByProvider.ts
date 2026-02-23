export type ProviderGroup<T extends { provider: string }> = {
  provider: string;
  label: string;
  models: T[];
};

const PROVIDER_LABELS: Record<string, string> = {
  anthropic: "Anthropic",
  openai: "OpenAI",
  google: "Google",
  mistral: "Mistral",
  groq: "Groq",
  xai: "xAI",
  deepseek: "DeepSeek",
  cohere: "Cohere",
  perplexity: "Perplexity",
  fireworks: "Fireworks",
  together: "Together",
  openrouter: "OpenRouter",
  bedrock: "AWS Bedrock",
  azure: "Azure OpenAI",
  vertex: "Google Vertex",
};

function formatProviderLabel(provider: string): string {
  if (!provider) return "Other";
  return PROVIDER_LABELS[provider] ?? provider.charAt(0).toUpperCase() + provider.slice(1);
}

export function groupModelsByProvider<T extends { provider: string }>(
  models: T[],
): ProviderGroup<T>[] {
  const groups = new Map<string, T[]>();
  for (const model of models) {
    const key = model.provider || "";
    const group = groups.get(key);
    if (group) {
      group.push(model);
    } else {
      groups.set(key, [model]);
    }
  }
  return Array.from(groups, ([provider, models]) => ({
    provider,
    label: formatProviderLabel(provider),
    models,
  }));
}
