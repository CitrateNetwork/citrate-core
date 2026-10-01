// =====================================================================
// citrate-core — HUP-S8.1 "Your devices" view model (pure, unit-tested).
//
// Turns the device-link list core returns into what the Cluster surface renders, and validates a
// device name with the SAME rule core and cluster-core enforce (1 to 48 ASCII letters, digits,
// spaces or . _ - ', no leading/trailing space), so the button never offers a name core refuses.
// =====================================================================
import type { DeviceLinks } from "../bridge/domains";

export const MAX_DEVICE_NAME = 48;

/** Null when `name` is a valid device name, else a short reason for the person. */
export function deviceNameError(name: string): string | null {
  if (name.length === 0) return "Give this device a name.";
  if (name.length > MAX_DEVICE_NAME) return `Keep the name to ${MAX_DEVICE_NAME} characters.`;
  if (name.trim() !== name) return "Remove the space at the start or end.";
  if (!/^[A-Za-z0-9 ._'-]+$/.test(name)) return "Use letters, digits, spaces, or . _ - ' only.";
  return null;
}

export interface DeviceRow {
  device: string;
  label: string;
  thisDevice: boolean;
}

export interface DevicePanelModel {
  thisDeviceLinked: boolean;
  thisDeviceLabel: string | null;
  rows: DeviceRow[];
}

/** The rows for "Your devices": this machine first, then the rest by index. */
export function devicePanelModel(links: DeviceLinks | null): DevicePanelModel {
  if (!links) return { thisDeviceLinked: false, thisDeviceLabel: null, rows: [] };
  const sorted = [...links.links].sort((a, b) =>
    a.thisDevice === b.thisDevice ? a.index - b.index : a.thisDevice ? -1 : 1,
  );
  const mine = sorted.find((l) => l.thisDevice) ?? null;
  return {
    thisDeviceLinked: mine !== null,
    thisDeviceLabel: mine?.label ?? null,
    rows: sorted.map((l) => ({ device: l.device, label: l.label, thisDevice: l.thisDevice })),
  };
}
