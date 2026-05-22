export interface ProviderDraftForUpdate {
  base_url: string;
  api_key: string;
  model: string;
  system_prompt: string;
  user_prompt: string;
  extra_params: Record<string, unknown>;
}

export interface UpdateProviderCommandArgs {
  name: string;
  baseUrl: string;
  apiKey: string;
  model: string;
  systemPrompt: string;
  userPrompt: string;
  extraParams: Record<string, unknown>;
}

export function buildUpdateProviderArgs(
  name: string,
  draft: ProviderDraftForUpdate
): UpdateProviderCommandArgs {
  return {
    name,
    baseUrl: draft.base_url,
    apiKey: draft.api_key,
    model: draft.model,
    systemPrompt: draft.system_prompt,
    userPrompt: draft.user_prompt,
    extraParams: draft.extra_params,
  };
}
