/**
 * AI configuration and actions.
 *
 * Note what is absent: there is no way to read an API key back. The backend
 * keeps it in the OS credential store and answers only whether one exists.
 */

import { invoke } from './ipc';
import type { Page } from '$lib/types/manuscript';

export type AiProvider = {
  id: string;
  name: string;
  baseUrl: string;
  model: string;
  /** Names the credential-store entry. Never the key. */
  credentialRef: string;
  temperature: number;
  maxOutputTokens: number | null;
  contextLength: number;
  /** Whether a key is saved. The key itself never crosses this boundary. */
  hasKey: boolean;
  createdAt: string;
  updatedAt: string;
};

export type ContextPolicy = 'selection' | 'page' | 'chapter';

export type AiProfile = {
  id: string;
  name: string;
  description: string;
  systemPrompt: string;
  providerId: string | null;
  temperature: number | null;
  contextPolicy: ContextPolicy;
  builtin: boolean;
  position: number;
};

export type AiActionId =
  | 'review'
  | 'critique'
  | 'tighten'
  | 'continuity'
  | 'questions'
  | 'alternatives';

export type AiActionInfo = {
  id: AiActionId;
  label: string;
  proposesAnEdit: boolean;
  kind: string;
};

/** What a request would send, shown to the writer before anything is sent. */
export type BuiltContext = {
  selection: string;
  surrounding: string;
  whereFrom: string;
  charsSent: number;
  trimmed: boolean;
  summary: string;
};

export type SuggestionStatus = 'pending' | 'applied' | 'dismissed' | 'stale';

export type AiSuggestion = {
  id: string;
  pageId: string;
  annotationId: string | null;
  baseRevision: number;
  anchorFrom: number;
  anchorTo: number;
  originalText: string;
  replacementText: string;
  contextHash: string;
  status: SuggestionStatus;
  createdAt: string;
  appliedAt: string | null;
};

// --- Configuration ----------------------------------------------------------

export function listProviders(): Promise<AiProvider[]> {
  return invoke('ai_providers');
}

/**
 * Saves a provider.
 *
 * `apiKey` is sent only when the writer typed a new one; omitting it leaves the
 * saved key alone, so changing a model name does not require re-entering it.
 */
export function saveProvider(input: {
  id?: string | null;
  name: string;
  baseUrl: string;
  model: string;
  temperature: number;
  maxOutputTokens?: number | null;
  contextLength: number;
  apiKey?: string | null;
}): Promise<AiProvider> {
  return invoke('ai_provider_save', {
    id: input.id ?? null,
    name: input.name,
    baseUrl: input.baseUrl,
    model: input.model,
    temperature: input.temperature,
    maxOutputTokens: input.maxOutputTokens ?? null,
    contextLength: input.contextLength,
    apiKey: input.apiKey ?? null
  });
}

export function deleteProvider(id: string): Promise<void> {
  return invoke('ai_provider_delete', { id });
}

export function listProfiles(): Promise<AiProfile[]> {
  return invoke('ai_profiles');
}

export function saveProfile(input: {
  id: string;
  name: string;
  description: string;
  systemPrompt: string;
  contextPolicy: ContextPolicy;
  temperature?: number | null;
}): Promise<AiProfile> {
  return invoke('ai_profile_save', {
    id: input.id,
    name: input.name,
    description: input.description,
    systemPrompt: input.systemPrompt,
    contextPolicy: input.contextPolicy,
    temperature: input.temperature ?? null
  });
}

export function deleteProfile(id: string): Promise<void> {
  return invoke('ai_profile_delete', { id });
}

export function listActions(): Promise<AiActionInfo[]> {
  return invoke('ai_actions');
}

// --- Running ----------------------------------------------------------------

/** Describes what an action would send, without sending anything. */
export function previewContext(
  pageId: string,
  profileId: string,
  action: AiActionId,
  from: number,
  to: number
): Promise<BuiltContext> {
  return invoke('ai_preview', { pageId, profileId, action, from, to });
}

/** Starts a request. Resolves to its id; output arrives as `ai:*` events. */
export function runAction(
  pageId: string,
  profileId: string,
  action: AiActionId,
  from: number,
  to: number
): Promise<string> {
  return invoke('ai_run', { pageId, profileId, action, from, to });
}

export function cancelRequest(requestId: string): Promise<boolean> {
  return invoke('ai_cancel', { requestId });
}

// --- Suggestions ------------------------------------------------------------

export function listSuggestions(pageId: string): Promise<AiSuggestion[]> {
  return invoke('ai_suggestions', { pageId });
}

/** Re-checks a suggestion against the Page without changing anything. */
export function reevaluateSuggestion(id: string): Promise<AiSuggestion> {
  return invoke('ai_suggestion_reevaluate', { id });
}

/**
 * Applies a suggestion.
 *
 * The backend re-validates first and refuses if the text has changed, so a
 * rejection here means the manuscript was left alone.
 */
export function applySuggestion(id: string): Promise<Page> {
  return invoke('ai_suggestion_apply', { id });
}

export function dismissSuggestion(id: string): Promise<AiSuggestion> {
  return invoke('ai_suggestion_dismiss', { id });
}
