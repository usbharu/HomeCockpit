export type CommandTransport = "udp" | "tcp";

export type DcsBiosConnectionConfig = {
  exportHost: string;
  exportPort: number;
  commandHost: string;
  commandPort: number;
  commandTransport: CommandTransport;
};

export type DcsBiosStatus = {
  connectionState: string;
  lastSeenAt: string | null;
  lastPacketAt: string | null;
  packetsPerSecond: number;
  totalPackets: number;
  aircraftName: string | null;
  error: string | null;
  diagnostics: string[];
};

export type AdapterCatalogState = "loaded" | "error";

export type AdapterArgumentOption = {
  value: string;
  label: string;
};

export type AdapterInputDefinition = {
  inputId: string;
  interface: string;
  description: string;
  maxValue: number | null;
  suggestedStep: number | null;
  argumentOptions: AdapterArgumentOption[];
  supportsEventValue: boolean;
};

export type AdapterOutputDefinition = {
  outputId: string;
  outputType: string;
  description: string;
  address: number;
  length: number | null;
  mask: number | null;
  shiftBy: number | null;
  maxValue: number | null;
  suffix: string;
};

export type AdapterControlDefinition = {
  category: string;
  controlId: string;
  controlType: string;
  description: string;
  positions: string[];
  inputs: AdapterInputDefinition[];
  outputs: AdapterOutputDefinition[];
};

export type AdapterProfile = {
  profileId: string;
  label: string;
  controlCount: number;
  controls: AdapterControlDefinition[];
};

export type AdapterDefinition = {
  adapterId: string;
  label: string;
  profiles: AdapterProfile[];
};

export type AdapterCatalog = {
  state: AdapterCatalogState;
  adapters: AdapterDefinition[];
  error: string | null;
};

export type ManagerLogEntry = {
  id: number;
  at: string;
  level: "INFO" | "WARN" | "ERROR" | "SUCCESS" | string;
  source: string;
  message: string;
};

export type DeviceEndpointTransport = "serial";

export type EndpointRoleHint = "auto" | "direct-device" | "imcp-hub";

export type DeviceEndpointConfig = {
  id: string;
  name: string;
  transport: DeviceEndpointTransport;
  address: string;
  enabled: boolean;
  baudRate: number;
  roleHint: EndpointRoleHint;
};

export type ManagedDeviceSummary = {
  id: string;
  connectionKind: "direct" | "hub" | "hub-child" | string;
  gatewayId: string | null;
  gatewayDisplayName: string | null;
  endpointId: string;
  endpointName: string;
  endpointTransport: string;
  endpointAddress: string;
  displayName: string;
  firmwareVersion: string | null;
  state: string;
  protocol: string;
  assignedAddress: number | null;
  deviceKind: string | null;
  protocolVersion: number | null;
  deviceId: string | null;
  displays: number | null;
  controls: number | null;
  features: string | null;
  deviceKindId: string | null;
};

export type EventKind =
  | "button-down"
  | "button-up"
  | "button-pushed"
  | "encoder-delta"
  | "absolute-changed"
  | "toggle-on"
  | "toggle-off";

export type RoleControlDefinition = {
  logicalControlId: string;
  label: string;
  supportedEvents: EventKind[];
};

export type RoleDefinition = {
  roleId: string;
  version: number;
  controls: RoleControlDefinition[];
};

export type PhysicalToLogicalBinding = {
  physicalControlId: number;
  logicalControlId: string;
};

export type DeviceRoleAssignment = {
  deviceId: string;
  roleId: string;
  bindings: PhysicalToLogicalBinding[];
};

export type AdapterActionConfig = {
  actionId: string;
  parameters: Record<string, string>;
};

export type AdapterControlMapping = {
  roleId: string;
  logicalControlId: string;
  eventKind: EventKind;
  action: AdapterActionConfig;
};

export type AdapterOutputMapping = {
  roleId: string;
  logicalControlId: string;
  sourceId: string;
  parameters: Record<string, string>;
};

export type AdapterMappingConfig = {
  adapterId: string;
  profileId: string;
  mappings: AdapterControlMapping[];
  outputMappings?: AdapterOutputMapping[];
};

export type LearnSessionStatus = {
  active: boolean;
  roleId: string | null;
  logicalControlId: string | null;
  targetDeviceId: string | null;
  expectedEventKind: EventKind | null;
  mode: "append" | "replace" | null;
  armedAt: string | null;
  timeoutMs: number;
  capturedDeviceId: string | null;
  capturedPhysicalControlId: number | null;
};

export type AppSnapshot = {
  dcsbiosConfig: DcsBiosConnectionConfig;
  dcsbiosStatus: DcsBiosStatus;
  adapterCatalog: AdapterCatalog;
  logs: ManagerLogEntry[];
  devices: ManagedDeviceSummary[];
  deviceEndpoints: DeviceEndpointConfig[];
  deviceRoleAssignments: DeviceRoleAssignment[];
  adapterMappings: AdapterMappingConfig[];
  roleDefinitions: RoleDefinition[];
  learnSession: LearnSessionStatus;
};

export type DcsBiosCommandRequest = {
  rawCommand?: string | null;
  controlId?: string | null;
  argument?: string | null;
};

export type LearnRequest = {
  roleId: string;
  logicalControlId: string;
  targetDeviceId?: string | null;
  expectedEventKind?: EventKind | null;
  mode: "append" | "replace";
  timeoutMs?: number;
};

export function defaultRoleDefinitions(): RoleDefinition[] {
  return ["left-ddi", "right-ddi"].map((roleId) => ({
    roleId,
    version: 1,
    controls: [],
  }));
}

export const defaultSnapshot: AppSnapshot = {
  dcsbiosConfig: {
    exportHost: "239.255.50.10",
    exportPort: 5010,
    commandHost: "127.0.0.1",
    commandPort: 7778,
    commandTransport: "udp",
  },
  dcsbiosStatus: {
    connectionState: "stopped",
    lastSeenAt: null,
    lastPacketAt: null,
    packetsPerSecond: 0,
    totalPackets: 0,
    aircraftName: null,
    error: null,
    diagnostics: ["DCS-BIOS listener is stopped."],
  },
  adapterCatalog: {
    state: "error",
    adapters: [],
    error: "Adapter catalog is not available.",
  },
  logs: [],
  devices: [],
  deviceEndpoints: [],
  deviceRoleAssignments: [],
  adapterMappings: [],
  roleDefinitions: defaultRoleDefinitions(),
  learnSession: {
    active: false,
    roleId: null,
    logicalControlId: null,
    targetDeviceId: null,
    expectedEventKind: null,
    mode: null,
    armedAt: null,
    timeoutMs: 0,
    capturedDeviceId: null,
    capturedPhysicalControlId: null,
  },
};
