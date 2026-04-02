import type { DeviceRole, NormalizedControlEvent } from "@/lib/manager-types";

export type ControlDefinition = {
  controlId: number;
  label: string;
  description: string;
  supportedEvents: NormalizedControlEvent[];
};

const buttonEvents: NormalizedControlEvent[] = ["BUTTON_DOWN", "BUTTON_UP", "BUTTON_PUSHED"];

function buildUpperPanelDdiCatalog(): ControlDefinition[] {
  return Array.from({ length: 40 }, (_, index) => {
    const row = Math.floor(index / 5) + 1;
    const column = (index % 5) + 1;

    return {
      controlId: index,
      label: `Button ${index + 1}`,
      description: `Matrix row ${row}, column ${column}`,
      supportedEvents: buttonEvents,
    };
  });
}

const catalogByDeviceKindId: Record<string, ControlDefinition[]> = {
  "upper-panel-ddi": buildUpperPanelDdiCatalog(),
};

export const deviceRoleLabels: Record<DeviceRole, string> = {
  "left-ddi": "LEFT_DDI",
  "right-ddi": "RIGHT_DDI",
};

export function getControlCatalog(deviceKindId: string | null): ControlDefinition[] {
  if (!deviceKindId) {
    return [];
  }

  return catalogByDeviceKindId[deviceKindId] ?? [];
}
