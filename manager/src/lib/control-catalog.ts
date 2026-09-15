import {
  defaultRoleDefinitions,
  type AdapterCatalog,
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

function buildUpperPanelDdiCatalog(controlCount: number): PhysicalControlDefinition[] {
  return Array.from({ length: controlCount }, (_, index) => {
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

function buildButtonPanelCatalog(controlCount: number): PhysicalControlDefinition[] {
  return Array.from({ length: controlCount }, (_, index) => ({
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

export const deviceRoleLabels: Record<string, string> = {
  "left-ddi": "LEFT_DDI",
  "right-ddi": "RIGHT_DDI",
};

export function getPhysicalControlCatalog(
  deviceKindId: string | null,
  controlCount: number | null,
): PhysicalControlDefinition[] {
  if (!deviceKindId || controlCount === null || controlCount <= 0) {
    return [];
  }

  switch (deviceKindId) {
    case "upper-panel-ddi":
      return buildUpperPanelDdiCatalog(controlCount);
    case "button-panel":
      return buildButtonPanelCatalog(controlCount);
    default:
      return [];
  }
}

export function getRoleDefinition(
  roleId: string,
  roleDefinitions: RoleDefinition[] = defaultRoleDefinitions(),
): RoleDefinition | null {
  return roleDefinitions.find((definition) => definition.roleId === roleId) ?? null;
}

export function getImplementedRoleControls(
  roleDefinition: RoleDefinition | null,
  controlCount: number,
): RoleDefinition["controls"] {
  if (!roleDefinition) {
    return [];
  }

  if (roleDefinition.controls.length > 0) {
    return roleDefinition.controls;
  }

  if (controlCount <= 0) {
    return [];
  }

  return Array.from({ length: controlCount }, (_, index) => ({
    logicalControlId: `button-${index}`,
    label: `Button ${index + 1}`,
    supportedEvents: [...buttonEvents],
  }));
}

export function getRoleDefinitions(
  roleDefinitions: RoleDefinition[] = defaultRoleDefinitions(),
): RoleDefinition[] {
  return roleDefinitions;
}

export function normalizeAircraftName(value: string): string {
  return value.trim().toLowerCase().replace(/[_\-/\s]/g, "");
}

export function findAdapterProfileForAircraft(
  adapterCatalog: AdapterCatalog,
  aircraftName: string | null,
) {
  if (!aircraftName) {
    return null;
  }

  const normalizedAircraftName = normalizeAircraftName(aircraftName);
  for (const adapter of adapterCatalog.adapters) {
    const profile = adapter.profiles.find((candidate) =>
      candidate.aircraftNames.some(
        (name) => normalizeAircraftName(name) === normalizedAircraftName,
      ),
    );
    if (profile) {
      return { adapter, profile };
    }
  }

  return null;
}
