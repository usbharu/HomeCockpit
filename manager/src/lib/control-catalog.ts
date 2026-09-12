import {
  defaultRoleDefinitions,
  type EventKind,
  type RoleDefinition,
} from "@/lib/manager-types";

export type PhysicalControlDefinition = {
  physicalControlId: number;
  label: string;
  description: string;
  supportedEvents: EventKind[];
};

const buttonEvents: EventKind[] = ["button-down", "button-up", "button-pushed"];

function buildUpperPanelDdiCatalog(): PhysicalControlDefinition[] {
  return Array.from({ length: 40 }, (_, index) => {
    const row = Math.floor(index / 5) + 1;
    const column = (index % 5) + 1;

    return {
      physicalControlId: index,
      label: `Button ${index + 1}`,
      description: `Matrix row ${row}, column ${column}`,
      supportedEvents: [...buttonEvents],
    };
  });
}

function buildButtonPanelCatalog(): PhysicalControlDefinition[] {
  return Array.from({ length: 64 }, (_, index) => ({
    physicalControlId: index,
    label: `Control ${index + 1}`,
    description: "Device-reported button panel control",
    supportedEvents: [
      ...buttonEvents,
      "encoder-delta",
      "absolute-changed",
      "toggle-on",
      "toggle-off",
    ],
  }));
}

const catalogByDeviceKindId: Record<string, PhysicalControlDefinition[]> = {
  "upper-panel-ddi": buildUpperPanelDdiCatalog(),
  "button-panel": buildButtonPanelCatalog(),
};

export const deviceRoleLabels: Record<string, string> = {
  "left-ddi": "LEFT_DDI",
  "right-ddi": "RIGHT_DDI",
};

export function getPhysicalControlCatalog(deviceKindId: string | null): PhysicalControlDefinition[] {
  if (!deviceKindId) {
    return [];
  }

  return catalogByDeviceKindId[deviceKindId] ?? [];
}

export function getRoleDefinition(
  roleId: string,
  roleDefinitions: RoleDefinition[] = defaultRoleDefinitions(),
): RoleDefinition | null {
  return roleDefinitions.find((definition) => definition.roleId === roleId) ?? null;
}

export function getRoleDefinitions(
  roleDefinitions: RoleDefinition[] = defaultRoleDefinitions(),
): RoleDefinition[] {
  return roleDefinitions;
}
