import type { Tag } from "../contracts";

export interface TagNode {
  tag: Tag;
  children: TagNode[];
}

/** One visible line of the Tags tree, with what an accessible tree needs to describe it. */
export interface TagRow {
  tag: Tag;
  /** 1 for the top level. */
  level: number;
  /** 1-based position among its siblings. */
  position: number;
  siblingCount: number;
  hasChildren: boolean;
  expanded: boolean;
  parentId: string | null;
}

const compareTags = (a: Tag, b: Tag) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true }) || a.id.localeCompare(b.id);

/**
 * Builds the Tag hierarchy from Immich's flat list. Siblings are sorted by name. A Tag whose parent
 * is not in the list is a top-level Tag, and so is a Tag that names itself as its parent. Tags that
 * form a cycle are all kept: the first of them (by name) is promoted to the top level so the cycle
 * is unrolled once.
 */
export function buildTagTree(tags: readonly Tag[]): TagNode[] {
  const byId = new Map<string, Tag>();
  for (const tag of tags) if (!byId.has(tag.id)) byId.set(tag.id, tag);
  const unique = [...byId.values()];

  const childrenByParent = new Map<string, Tag[]>();
  const roots: Tag[] = [];
  for (const tag of unique) {
    const parentId = tag.parentId;
    if (parentId && parentId !== tag.id && byId.has(parentId)) {
      const siblings = childrenByParent.get(parentId) ?? [];
      siblings.push(tag);
      childrenByParent.set(parentId, siblings);
    } else roots.push(tag);
  }

  const placed = new Set<string>();
  function build(tag: Tag): TagNode {
    placed.add(tag.id);
    const children = (childrenByParent.get(tag.id) ?? [])
      .filter((child) => !placed.has(child.id))
      .sort(compareTags)
      .map(build);
    return { tag, children };
  }

  const nodes = roots.sort(compareTags).map(build);

  // Whatever is still unplaced hangs below a cycle. Promote one Tag of each cycle to the top level.
  for (const tag of [...unique].sort(compareTags)) {
    if (placed.has(tag.id)) continue;
    nodes.push(build(cycleMember(tag, byId)));
  }
  return nodes.sort((a, b) => compareTags(a.tag, b.tag));
}

/** The first Tag that repeats when walking up from `start`; it is part of a cycle. */
function cycleMember(start: Tag, byId: Map<string, Tag>): Tag {
  const seen = new Set<string>();
  let current = start;
  while (!seen.has(current.id)) {
    seen.add(current.id);
    const parent = current.parentId ? byId.get(current.parentId) : undefined;
    if (!parent) return current;
    current = parent;
  }
  return current;
}

/**
 * The lines to show: every top-level Tag, and the children of each expanded Tag. `isExpanded`
 * is asked for Tags that have children only.
 */
export function visibleTagRows(nodes: readonly TagNode[], isExpanded: (tag: Tag, level: number) => boolean): TagRow[] {
  const rows: TagRow[] = [];
  function visit(siblings: readonly TagNode[], level: number, parentId: string | null) {
    siblings.forEach((node, index) => {
      const hasChildren = node.children.length > 0;
      const expanded = hasChildren && isExpanded(node.tag, level);
      rows.push({ tag: node.tag, level, position: index + 1, siblingCount: siblings.length, hasChildren, expanded, parentId });
      if (expanded) visit(node.children, level + 1, node.tag.id);
    });
  }
  visit(nodes, 1, null);
  return rows;
}

/** A Tag colour is shown only when it is exactly `#RRGGBB`. */
export function validTagColor(color: string | null | undefined): string | null {
  return color && /^#[0-9a-fA-F]{6}$/.test(color) ? color : null;
}
