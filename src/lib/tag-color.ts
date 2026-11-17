/** A Tag colour is shown only when it is exactly `#RRGGBB`. */
export function validTagColor(color: string | null | undefined): string | null {
  return color && /^#[0-9a-fA-F]{6}$/.test(color) ? color : null;
}
