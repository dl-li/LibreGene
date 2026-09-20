// Supplier (provider) data for enzymes, loaded once from get_enzyme_providers.
// All lookups are case-insensitive and match aliases as well as DB names.

import { getEnzymeProviders } from './tauriApi';

export const ENZYME_PROVIDER_OPTIONS = [
  { value: 'all', label: 'All Suppliers' },
  { value: 'neb', label: 'NEB' },
  { value: 'bestenzyme', label: '愚公 (Yugong)' },
  { value: 'thermo', label: 'Thermo FastDigest' },
];

export const ENZYME_PROVIDER_VALUES = new Set(ENZYME_PROVIDER_OPTIONS.map((o) => o.value));

export const PROVIDER_LABEL = {
  neb: 'NEB',
  bestenzyme: '愚公 (Yugong)',
  thermo: 'Thermo FastDigest',
};

// Provider section order in the detail dialog.
export const PROVIDER_ORDER = ['neb', 'bestenzyme', 'thermo'];

let loadPromise = null; // shared module-level cache; resolves to null on failure
export function loadProviderData() {
  if (!loadPromise) {
    loadPromise = getEnzymeProviders().catch(() => null);
  }
  return loadPromise;
}

// Map<lowercased name-or-alias, entry> where entry is
// { name, aliases, providers } from either `enzymes` or `providerOnly`.
export function buildProviderIndex(data) {
  const index = new Map();
  if (!data) return index;
  const put = (key, entry) => {
    const k = (key || '').toLowerCase();
    if (k && !index.has(k)) index.set(k, entry);
  };
  for (const [name, entry] of Object.entries(data.enzymes || {})) {
    put(name, entry);
    for (const alias of entry.aliases || []) put(alias, entry);
  }
  for (const entry of data.providerOnly || []) {
    put(entry.name, entry);
    for (const alias of entry.aliases || []) put(alias, entry);
  }
  return index;
}

export function findProviderEntry(index, enzymeName) {
  return index?.get((enzymeName || '').toLowerCase()) || null;
}

export function hasProvider(entry, providerKey) {
  return !!entry?.providers?.[providerKey];
}
