import type { SearchCamera } from "../contracts";
import { chosenParts, firstAndMore, setPart } from "./parts";
import type { SearchFilterDefinition } from "./types";

/** Camera: make, model and lens model, each optional and each chosen from Immich's suggestions. */
export const cameraFilter: SearchFilterDefinition = {
  key: "camera",
  apply({ camera }, request) {
    const chosen = chosenParts(camera, ["make", "model", "lensModel"]);
    if (chosen) request.camera = chosen;
  },
  summary({ camera }) {
    const body = [camera?.make, camera?.model].filter(Boolean).join(" ");
    const values = [body, camera?.lensModel].filter((value): value is string => Boolean(value));
    return values.length > 0 ? firstAndMore(values) : null;
  },
};

/**
 * The camera filter with one part set (or cleared, with `undefined`); unset when no part is left. A different
 * make drops the chosen model, which may not exist for that make (model suggestions are limited by the make).
 */
export function setCameraPart(camera: SearchCamera | undefined, part: keyof SearchCamera, value: string | undefined): SearchCamera | undefined {
  return setPart(camera, part, value, { parent: "make", dependent: "model" });
}
