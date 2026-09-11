import { useEffect, useRef, useState } from "react";

/**
 * Renders the app's `data-tooltip="..."` bubbles as a single position:fixed
 * element positioned via JS, instead of a per-element CSS `::after`.
 *
 * The CSS-only version (`::after`, `position: absolute`) broke inside any
 * scrolling container (e.g. the History toolbar's `overflow-x: auto`): even
 * though the bubble is invisible until hover, its layout box still counts
 * toward the scroll container's scrollable content — so a bubble centered on
 * a narrow icon button and wider than that button (e.g. "Compare selected
 * (2)" on a 32px button) silently pushed a horizontal scrollbar onto a row
 * that had plenty of visible room. `position: fixed`, positioned relative to
 * the viewport, never participates in an ancestor's scroll geometry.
 */
/** Below this much room above the trigger, flip the bubble to render below
 * it instead — enough for the bubble's own height (~28px) plus the gap. */
const MIN_SPACE_ABOVE = 40;

export default function DataTooltip() {
  const [state, setState] = useState<{
    text: string;
    top: number;
    left: number;
    placement: "above" | "below";
  } | null>(null);
  const bubbleRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let current: HTMLElement | null = null;

    const position = (target: HTMLElement, text: string) => {
      const rect = target.getBoundingClientRect();
      const placement: "above" | "below" =
        rect.top < MIN_SPACE_ABOVE ? "below" : "above";
      setState({
        text,
        top: placement === "above" ? rect.top - 7 : rect.bottom + 7,
        left: rect.left + rect.width / 2,
        placement,
      });
    };

    const show = (target: HTMLElement) => {
      const text = target.getAttribute("data-tooltip");
      if (!text) return;
      current = target;
      position(target, text);
    };

    const hide = (target: HTMLElement) => {
      if (current === target) {
        current = null;
        setState(null);
      }
    };

    const onOver = (e: Event) => {
      const target = (e.target as HTMLElement | null)?.closest(
        "[data-tooltip]"
      );
      if (target instanceof HTMLElement) show(target);
    };
    const onOut = (e: Event) => {
      const target = (e.target as HTMLElement | null)?.closest(
        "[data-tooltip]"
      );
      if (target instanceof HTMLElement) hide(target);
    };
    const onFocusIn = (e: FocusEvent) => {
      const target = (e.target as HTMLElement | null)?.closest(
        "[data-tooltip]"
      );
      if (target instanceof HTMLElement) show(target);
    };
    const onFocusOut = (e: FocusEvent) => {
      const target = (e.target as HTMLElement | null)?.closest(
        "[data-tooltip]"
      );
      if (target instanceof HTMLElement) hide(target);
    };
    const onScrollOrResize = () => {
      if (current) setState(null);
    };

    document.addEventListener("mouseover", onOver);
    document.addEventListener("mouseout", onOut);
    document.addEventListener("focusin", onFocusIn);
    document.addEventListener("focusout", onFocusOut);
    window.addEventListener("scroll", onScrollOrResize, true);
    window.addEventListener("resize", onScrollOrResize);
    return () => {
      document.removeEventListener("mouseover", onOver);
      document.removeEventListener("mouseout", onOut);
      document.removeEventListener("focusin", onFocusIn);
      document.removeEventListener("focusout", onFocusOut);
      window.removeEventListener("scroll", onScrollOrResize, true);
      window.removeEventListener("resize", onScrollOrResize);
    };
  }, []);

  if (!state) return null;

  const width = bubbleRef.current?.offsetWidth ?? 0;
  const left = Math.max(
    4,
    Math.min(state.left - width / 2, window.innerWidth - width - 4)
  );

  return (
    <div
      ref={bubbleRef}
      className="shared-tooltip"
      role="tooltip"
      style={{
        top: state.top,
        left,
        transform: state.placement === "above" ? "translateY(-100%)" : "none",
      }}
    >
      {state.text}
    </div>
  );
}
