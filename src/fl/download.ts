// citrate-core: save a text export (CSV, JSON) the member asked for, through the webview's
// download, as the Journal export does. Returns the sentence the view shows.
export async function downloadText(name: string, text: string, mime = "text/csv"): Promise<string> {
  const blob = new Blob([text], { type: mime });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 3000);
  return `Saved ${name} to Downloads`;
}
