import type { TimelineAsset } from "../contracts";

const MIN_TILE_WIDTH = 200;
const MIN_COLUMN_COUNT = 3;
const COLUMN_GAP = 8;

export interface MasonryItem {
  readonly asset: TimelineAsset;
  readonly height: number;
}

export interface MasonryLayout {
  readonly columnCount: number;
  readonly columnWidth: number;
  readonly columns: readonly (readonly MasonryItem[])[];
  readonly height: number;
}

function calculateColumnCount(containerWidth: number): number {
  return Math.max(
    MIN_COLUMN_COUNT,
    Math.floor((containerWidth + COLUMN_GAP) / (MIN_TILE_WIDTH + COLUMN_GAP)),
  );
}

function assetHeight(asset: TimelineAsset, columnWidth: number): number {
  if (asset.width <= 0 || asset.height <= 0) return columnWidth;
  return columnWidth * (asset.height / asset.width);
}

export function calculateMasonryLayout(assets: readonly TimelineAsset[], containerWidth: number): MasonryLayout {
  const columnCount = calculateColumnCount(containerWidth);
  const columnWidth = Math.max(1, (containerWidth - (columnCount - 1) * COLUMN_GAP) / columnCount);
  const columns: MasonryItem[][] = Array.from({ length: columnCount }, () => []);
  const columnHeights = Array.from({ length: columnCount }, () => 0);

  for (const asset of assets) {
    const columnIndex = columnHeights.indexOf(Math.min(...columnHeights));
    const height = assetHeight(asset, columnWidth);
    columns[columnIndex].push({ asset, height });
    columnHeights[columnIndex] += height + (columns[columnIndex].length > 1 ? COLUMN_GAP : 0);
  }

  return {
    columnCount,
    columnWidth,
    columns,
    height: Math.max(0, ...columnHeights),
  };
}
