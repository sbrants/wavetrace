export function installKindNote(kind: string): string {
  switch (kind) {
    case "microsoft_store":
      return (
        "Microsoft Store install — data lives under " +
        "%LOCALAPPDATA%\\Packages\\…\\LocalCache\\Roaming\\wavetrace\\, " +
        "not %APPDATA%\\wavetrace\\."
      );
    case "mac_sandbox":
      return (
        "Sandboxed Mac install — data is inside the app container at " +
        "~/Library/Containers/…/Data/Library/Application Support/wavetrace/."
      );
    case "mac_direct":
      return (
        "Direct download — typical Mac location is " +
        "~/Library/Application Support/wavetrace/."
      );
    case "windows_direct":
      return "Direct download — typical Windows location is %APPDATA%\\wavetrace\\.";
    case "linux_flatpak":
      return "Flatpak install — data is under ~/.var/app/…/data/wavetrace/.";
    case "linux_direct":
      return "Direct download — typical Linux location is ~/.local/share/wavetrace/.";
    default:
      return "Local app data:";
  }
}
