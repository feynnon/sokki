/** A point on the screen, in CSS px. */
export interface Point {
  x: number;
  y: number;
}

/** From this far the press moves the pill; below it, it is a click. */
export const DRAG_PX = 4;

/** What a press on the pill does once the mouse is at `to`: still a click,
 * or a drag that moves the pill. */
export function pressOutcome(from: Point, to: Point): "click" | "drag" {
  return Math.hypot(to.x - from.x, to.y - from.y) >= DRAG_PX ? "drag" : "click";
}
