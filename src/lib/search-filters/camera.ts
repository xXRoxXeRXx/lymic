import type { SearchCamera } from "../contracts";
import { setPart } from "./parts";
import type { SearchFilterDefinition } from "./types";

/** Camera: make, model and lens model, each optional and each chosen from Immich's suggestions. */
export const cameraFilter: SearchFilterDefinition = {
  key: "camera",
  apply({ camera }, request) {
    const chosen: SearchCamera = {};
    if (camera?.make) chosen.make = camera.make;
    if (camera?.model) chosen.model = camera.model;
    if (camera?.lensModel) chosen.lensModel = camera.lensModel;
    if (Object.keys(chosen).length > 0) request.camera = chosen;
  },
};

/**
 * The camera filter with one part set (or cleared, with `undefined`); unset when no part is left. A different
 * make drops the chosen model, which may not exist for that make (model suggestions are limited by the make).
 */
export function setCameraPart(camera: SearchCamera | undefined, part: keyof SearchCamera, value: string | undefined): SearchCamera | undefined {
  return setPart(camera, part, value, { parent: "make", dependent: "model" });
}
