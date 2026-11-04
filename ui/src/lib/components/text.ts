import type { I18n, MessageId, MessageParamsById } from '../i18n';

/** Message IDs without placeholders. */
type PlainMessageId = {
  [I in MessageId]: keyof MessageParamsById[I] extends never ? I : never;
}[MessageId];

/** Message IDs with placeholders: each keeps its own parameter type. */
type ParameterizedMessageId = Exclude<MessageId, PlainMessageId>;

/**
 * A message ID with its parameters. IDs without placeholders share one object type, so the union
 * has one member per parameterized ID only: a member per ID (about 1000) exceeds the TypeScript
 * limit for union subtype reduction ("union type that is too complex to represent").
 */
export type LocalizedText =
  | { readonly id: PlainMessageId; readonly params?: Readonly<Record<never, never>> }
  | {
      [I in ParameterizedMessageId]: { readonly id: I; readonly params: Readonly<MessageParamsById[I]> };
    }[ParameterizedMessageId];

export interface UserText {
  readonly value: string;
  readonly kind: 'user' | 'technical';
}

export type DisplayText = LocalizedText | UserText;

export function resolveText(i18n: I18n, text: DisplayText): string {
  if ('value' in text) return text.value;
  const translate = i18n.t as (id: MessageId, params?: object) => string;
  return translate(text.id, text.params);
}
