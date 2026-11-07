export interface CloneComparisonData {
  operation_id: string | null;
  observation: 'pending' | 'unavailable' | 'verified_read_back' | 'changed_since_read_back' | 'unverified';
  verdict: string | null;
  access: (CloneAccess | null)[];
  rows: CloneComparisonRow[];
}

interface CloneAccess {
  private: boolean | null;
  shared: boolean | null;
  owner_matches: boolean | null;
}

interface CloneComparisonRow {
  kind: 'dashboard' | 'page' | 'widget' | 'field';
  page: number | null;
  at: [string, string] | null;
  field: string | null;
  change: 'changed' | 'added' | 'untouched';
  unresolved: boolean;
  kept: boolean;
  observed_differs: boolean;
  source: CloneCell | null;
  expected: CloneCell | null;
  observed: CloneCell | null;
}

interface CloneCell {
  label: string | null;
  widget_type: string | null;
  geometry: [string, string, string, string] | null;
}
