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

export type DeviceRole = "left-ddi" | "right-ddi";

export type NormalizedControlEvent =
  | "BUTTON_DOWN"
  | "BUTTON_UP"
  | "BUTTON_PUSHED"
  | "ENCODER_DELTA"
  | "ABSOLUTE_CHANGED"
  | "TOGGLE_ON"
  | "TOGGLE_OFF";

export type DeviceRoleAssignment = {
  deviceId: string;
  role: DeviceRole;
};

export type DcsBiosMappedAction = {
  identifier: string;
  argument: string;
};

export type RoleControlMapping = {
  id: string;
  controlId: number;
  inputEvent: NormalizedControlEvent;
  action: DcsBiosMappedAction;
};

export type RoleMappingConfig = {
  role: DeviceRole;
  mappings: RoleControlMapping[];
};

export type AppSnapshot = {
  dcsbiosConfig: DcsBiosConnectionConfig;
  dcsbiosStatus: DcsBiosStatus;
  logs: ManagerLogEntry[];
  devices: ManagedDeviceSummary[];
  deviceEndpoints: DeviceEndpointConfig[];
  deviceRoleAssignments: DeviceRoleAssignment[];
  roleMappings: RoleMappingConfig[];
};

export type DcsBiosCommandRequest = {
  rawCommand?: string | null;
  controlId?: string | null;
  argument?: string | null;
};

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
  logs: [],
  devices: [],
  deviceEndpoints: [],
  deviceRoleAssignments: [],
  roleMappings: [],
};
