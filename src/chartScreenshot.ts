const PANEL_BG = "#16203a";
const TITLE_COLOR = "#8da2c0";

export async function captureChartCard(element: HTMLElement): Promise<Blob> {
  const svg = element.querySelector("svg");
  if (!svg) throw new Error("No chart to capture");

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

  const cloned = svg.cloneNode(true) as SVGSVGElement;
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

export async function copyChartScreenshot(blob: Blob) {
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
