import type { PageHostSelection, PageRole } from './client';

const slug = /^[a-z][a-z0-9_-]{1,63}$/;

export function validNewRole(id: string, role: PageRole, roles: Record<string, PageRole>): boolean {
  const slots = role.slots ?? [];
  const usedSlots = new Set(Object.values(roles).flatMap((item) => item.slots ?? []));
  return slug.test(id) && !Object.hasOwn(roles, id) && Object.keys(roles).length < 64
    && slots.length > 0 && slots.length <= 256 && (role.cardinality !== 'one' || slots.length === 1)
    && new Set(slots).size === slots.length && slots.every((slot) => slug.test(slot) && !usedSlots.has(slot));
}

export function validSource(hosts: PageHostSelection, roles: Record<string, PageRole>): boolean {
  const role = roles[hosts.role];
  if (!role?.slots?.length) return false;
  if (hosts.selection === 'slot') return !!hosts.slot_id && role.slots.includes(hosts.slot_id);
  return hosts.slot_id === undefined && (hosts.selection === 'all' || role.cardinality === 'one');
}

export function singleSource(hosts: PageHostSelection, roles: Record<string, PageRole>): boolean {
  return validSource(hosts, roles) && (hosts.selection === 'slot' || roles[hosts.role].cardinality === 'one');
}
