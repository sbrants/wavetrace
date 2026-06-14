import { invoke } from "@tauri-apps/api/core";

const PANEL_BG = "#16203a";
const TITLE_COLOR = "#8da2c0";

const STYLE_PROPS = [
  "fill",
  "stroke",
  "stroke-width",
  "opacity",
  "font-family",
  "font-size",
  "font-weight",
  "text-anchor",
  "dominant-baseline",
] as const;

function findChartSvg(root: HTMLElement): SVGSVGElement {
  const recharts = root.querySelector("svg.recharts-surface");
  if (recharts instanceof SVGSVGElement) {
    return recharts;
  }

  const candidates = [...root.querySelectorAll("svg")].filter(
    (svg) => !svg.closest(".chart-screenshot-actions")
  );
  if (candidates.length === 0) {
    throw new Error("No chart to capture");
  }

  return candidates.reduce((best, svg) => {
    const a = best.getBoundingClientRect();
    const b = svg.getBoundingClientRect();
    return b.width * b.height > a.width * a.height ? svg : best;
  });
}

function prepareSvgForExport(source: SVGSVGElement): SVGSVGElement {
  const cloned = source.cloneNode(true) as SVGSVGElement;
  cloned.setAttribute("xmlns", "http://www.w3.org/2000/svg");
  cloned.setAttribute("xmlns:xlink", "http://www.w3.org/1999/xlink");

  const sourceNodes = [source, ...source.querySelectorAll("*")];
  const cloneNodes = [cloned, ...cloned.querySelectorAll("*")];
  cloneNodes.forEach((node, i) => {
    const el = node as SVGElement;
    const src = sourceNodes[i] as Element;
    const computed = window.getComputedStyle(src);
    for (const prop of STYLE_PROPS) {
      const value = computed.getPropertyValue(prop);
      if (value && value !== "none" && value !== "normal") {
        el.style.setProperty(prop, value);
      }
    }
  });

  return cloned;
}

export async function captureChartCard(element: HTMLElement): Promise<Blob> {
  const svg = findChartSvg(element);

  const title = element.querySelector("h3")?.textContent ?? "chart";
  const svgRect = svg.getBoundingClientRect();
  if (svgRect.width === 0 || svgRect.height === 0) {
    throw new Error("Chart is not visible");
  }

  const padding = 16;
  const titleHeight = 28;
  const scale = 2;

  const canvas = document.createElement("canvas");
  canvas.width = (svgRect.width + padding * 2) * scale;
  canvas.height = (svgRect.height + titleHeight + padding * 2) * scale;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Canvas not supported");

  ctx.fillStyle = PANEL_BG;
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  ctx.fillStyle = TITLE_COLOR;
  ctx.font = `${14 * scale}px "Segoe UI", system-ui, sans-serif`;
  ctx.fillText(title, padding * scale, (padding + 14) * scale);

  const cloned = prepareSvgForExport(svg);
  cloned.setAttribute("width", String(svgRect.width));
  cloned.setAttribute("height", String(svgRect.height));

  const svgData = new XMLSerializer().serializeToString(cloned);
  const svgBlob = new Blob([svgData], { type: "image/svg+xml;charset=utf-8" });
  const url = URL.createObjectURL(svgBlob);

  try {
    await new Promise<void>((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        ctx.drawImage(
          img,
          padding * scale,
          (titleHeight + padding) * scale,
          svgRect.width * scale,
          svgRect.height * scale
        );
        resolve();
      };
      img.onerror = () => reject(new Error("Failed to render chart"));
      img.src = url;
    });
  } finally {
    URL.revokeObjectURL(url);
  }

  const blob = await new Promise<Blob | null>((resolve) =>
    canvas.toBlob(resolve, "image/png")
  );
  if (!blob) throw new Error("Capture failed");
  return blob;
}

export function chartScreenshotFilename(title: string): string {
  const slug =
    title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "")
      .slice(0, 60) || "chart";
  const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  return `${slug}-${stamp}.png`;
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const dataUrl = reader.result as string;
      resolve(dataUrl.slice(dataUrl.indexOf(",") + 1));
    };
    reader.onerror = () => reject(new Error("Failed to encode image"));
    reader.readAsDataURL(blob);
  });
}

export async function copyChartScreenshot(blob: Blob) {
  if ("__TAURI_INTERNALS__" in window) {
    const pngBase64 = await blobToBase64(blob);
    await invoke("copy_image_to_clipboard", { pngBase64 });
    return;
  }

  if (!navigator.clipboard?.write) {
    throw new Error("Clipboard is not available");
  }
  await navigator.clipboard.write([
    new ClipboardItem({ "image/png": blob }),
  ]);
}

export function downloadChartScreenshot(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  try {
    const link = document.createElement("a");
    link.href = url;
    link.download = filename;
    link.click();
  } finally {
    URL.revokeObjectURL(url);
  }
}
