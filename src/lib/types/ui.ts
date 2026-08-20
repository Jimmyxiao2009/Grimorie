import type { IconName } from '$lib/design/icons';

/** One row of a contextual menu. */
export type MenuAction = {
  kind?: 'action';
  id: string;
  label: string;
  icon?: IconName;
  /** Right-aligned hint, normally a keyboard shortcut. */
  hint?: string;
  danger?: boolean;
  disabled?: boolean;
  select: () => void;
};

export type MenuSeparator = { kind: 'separator'; id: string };
export type MenuHeading = { kind: 'heading'; id: string; label: string };

export type MenuItem = MenuAction | MenuSeparator | MenuHeading;

export function isMenuAction(item: MenuItem): item is MenuAction {
  return item.kind === undefined || item.kind === 'action';
}

/** Where a dropdown prefers to sit relative to its trigger, before flipping. */
export type MenuPlacement = 'bottom-start' | 'bottom-end' | 'top-start' | 'top-end';
