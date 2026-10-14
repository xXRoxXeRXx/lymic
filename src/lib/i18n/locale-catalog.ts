import catalog from './locale-catalog.json';

export interface LocaleCatalogEntry {
  code: string;
  name: string;
  bcp47: string;
  translationSource: string;
}

export const localeCatalog: readonly LocaleCatalogEntry[] = catalog;
