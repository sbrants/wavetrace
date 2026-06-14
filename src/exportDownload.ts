export function downloadBlob(blob: Blob, filename: string) {
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

export function downloadTextFile(
  content: string,
  filename: string,
  mime = "text/csv;charset=utf-8"
) {
  downloadBlob(new Blob([content], { type: mime }), filename);
}

export function downloadBase64File(
  dataBase64: string,
  filename: string,
  mime: string
) {
  const binary = atob(dataBase64);
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  downloadBlob(new Blob([bytes], { type: mime }), filename);
}
