import type { I18n, MessageId, MessageParamsById } from '../i18n';

export type LocalizedText = {
  [I in MessageId]: keyof MessageParamsById[I] extends never
    ? { readonly id: I; readonly params?: Readonly<MessageParamsById[I]> }
    : { readonly id: I; readonly params: Readonly<MessageParamsById[I]> };
}[MessageId];

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
