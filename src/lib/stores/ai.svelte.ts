/**
 * AI state: configuration, the request in flight, and suggestions.
 *
 * Only one request runs at a time. A writer asking two questions about the same
 * passage at once is far more likely to be an accidental double-press than an
 * intention, and one visible stream is easier to reason about than several.
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import * as service from '$lib/services/ai';
import { describeError, isDesktop } from '$lib/services/ipc';
import { notices } from './notices.svelte';
import type {
  AiActionId,
  AiActionInfo,
  AiProfile,
  AiProvider,
  AiSuggestion,
  BuiltContext
} from '$lib/services/ai';

type DeltaEvent = { requestId: string; delta?: string };
type DoneEvent = { requestId: string; annotationId: string; suggestionId?: string };
type FailedEvent = { requestId: string; message: string; code: string };

class AiStore {
  providers = $state<AiProvider[]>([]);
  profiles = $state<AiProfile[]>([]);
  actions = $state<AiActionInfo[]>([]);
  suggestions = $state<AiSuggestion[]>([]);

  /** The request currently streaming, if any. */
  requestId = $state<string | null>(null);
  /** Text as it arrives, so the writer sees the answer forming. */
  streamed = $state('');
  /** What the pending request is asking, for the panel's heading. */
  running = $state<{ profileName: string; actionLabel: string } | null>(null);

  /** Set when a run finishes, so the Margin can scroll to the new note. */
  lastAnnotationId = $state<string | null>(null);

  readonly isConfigured = $derived(this.providers.some((provider) => provider.hasKey));
  readonly isStreaming = $derived(this.requestId !== null);

  private unlisten: UnlistenFn[] = [];
  private onfinished?: () => void;

  /** Subscribes to streaming events. Returns a teardown. */
  start(onfinished: () => void): () => void {
    this.onfinished = onfinished;
    if (!isDesktop) return () => {};

    void listen<DeltaEvent>('ai:delta', ({ payload }) => {
      if (payload.requestId !== this.requestId) return;
      this.streamed += payload.delta ?? '';
    }).then((off) => this.unlisten.push(off));

    void listen<DoneEvent>('ai:done', ({ payload }) => {
      if (payload.requestId !== this.requestId) return;
      this.lastAnnotationId = payload.annotationId;
      this.finish();
    }).then((off) => this.unlisten.push(off));

    void listen<FailedEvent>('ai:failed', ({ payload }) => {
      if (payload.requestId !== this.requestId) return;
      notices.failure(new Error(payload.message));
      this.finish();
    }).then((off) => this.unlisten.push(off));

    return () => {
      for (const off of this.unlisten) off();
      this.unlisten = [];
      this.onfinished = undefined;
    };
  }

  private finish(): void {
    this.requestId = null;
    this.streamed = '';
    this.running = null;
    this.onfinished?.();
  }

  async loadConfiguration(): Promise<void> {
    try {
      const [providers, profiles, actions] = await Promise.all([
        service.listProviders(),
        service.listProfiles(),
        service.listActions()
      ]);
      this.providers = providers;
      this.profiles = profiles;
      this.actions = actions;
    } catch {
      // AI configuration failing to load must not stop someone writing. The
      // AI controls simply stay unavailable.
      this.providers = [];
      this.profiles = [];
      this.actions = [];
    }
  }

  async loadSuggestions(pageId: string | null): Promise<void> {
    if (!pageId) {
      this.suggestions = [];
      return;
    }
    try {
      this.suggestions = await service.listSuggestions(pageId);
    } catch {
      this.suggestions = [];
    }
  }

  suggestionFor(annotationId: string): AiSuggestion | null {
    return this.suggestions.find((s) => s.annotationId === annotationId) ?? null;
  }

  /** What a request would send. Shown before anything leaves the machine. */
  preview(
    pageId: string,
    profileId: string,
    action: AiActionId,
    from: number,
    to: number
  ): Promise<BuiltContext> {
    return service.previewContext(pageId, profileId, action, from, to);
  }

  async run(
    pageId: string,
    profile: AiProfile,
    action: AiActionInfo,
    from: number,
    to: number
  ): Promise<void> {
    if (this.isStreaming) return;

    this.streamed = '';
    this.running = { profileName: profile.name, actionLabel: action.label };
    try {
      this.requestId = await service.runAction(pageId, profile.id, action.id, from, to);
    } catch (error) {
      this.running = null;
      this.requestId = null;
      notices.failure(error);
    }
  }

  async cancel(): Promise<void> {
    const id = this.requestId;
    if (!id) return;
    // Cleared immediately: the writer pressed stop, and the UI should say so
    // without waiting for the backend to confirm.
    this.finish();
    try {
      await service.cancelRequest(id);
    } catch {
      // The request had already finished. Nothing to report.
    }
  }

  async applySuggestion(id: string): Promise<boolean> {
    try {
      await service.applySuggestion(id);
      return true;
    } catch (error) {
      // A refusal here means the manuscript was left alone, which is the
      // important part of the message and is already in it.
      notices.failure(error);
      await this.refreshSuggestion(id);
      return false;
    }
  }

  async dismissSuggestion(id: string): Promise<void> {
    try {
      const updated = await service.dismissSuggestion(id);
      this.replace(updated);
    } catch (error) {
      notices.failure(error);
    }
  }

  async reevaluateSuggestion(id: string): Promise<AiSuggestion | null> {
    try {
      const updated = await service.reevaluateSuggestion(id);
      this.replace(updated);
      notices.info(
        updated.status === 'pending'
          ? 'This suggestion still fits the text.'
          : 'The text has changed, so this suggestion no longer fits.'
      );
      return updated;
    } catch (error) {
      notices.failure(error);
      return null;
    }
  }

  private async refreshSuggestion(id: string): Promise<void> {
    const existing = this.suggestions.find((s) => s.id === id);
    if (existing) await this.loadSuggestions(existing.pageId);
  }

  private replace(updated: AiSuggestion): void {
    this.suggestions = this.suggestions.map((s) => (s.id === updated.id ? updated : s));
  }

  clear(): void {
    this.suggestions = [];
    this.streamed = '';
    this.requestId = null;
    this.running = null;
    this.lastAnnotationId = null;
  }

  /** Reloads configuration after Settings changed it. */
  async refreshConfiguration(): Promise<void> {
    await this.loadConfiguration();
  }

  describe(error: unknown): string {
    return describeError(error);
  }
}

export const ai = new AiStore();
