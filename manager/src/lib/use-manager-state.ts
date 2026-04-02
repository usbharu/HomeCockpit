"use client";

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

import { isTauri } from "@/lib/is-tauri";
import {
  defaultSnapshot,
  type AppSnapshot,
  type DcsBiosCommandRequest,
  type DcsBiosConnectionConfig,
  type DcsBiosStatus,
  type DeviceRoleAssignment,
  type DeviceEndpointConfig,
  type ManagerLogEntry,
  type ManagedDeviceSummary,
  type RoleMappingConfig,
} from "@/lib/manager-types";

export function useManagerState() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>(defaultSnapshot);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [serialPorts, setSerialPorts] = useState<string[]>([]);

  const replaceSnapshot = useCallback((next: AppSnapshot) => {
    setSnapshot(next);
    setRuntimeError(null);
  }, []);

  const mergeStatus = useCallback((status: DcsBiosStatus) => {
    setSnapshot((current) => ({ ...current, dcsbiosStatus: status }));
  }, []);

  const mergeLog = useCallback((entry: ManagerLogEntry) => {
    setSnapshot((current) => ({
      ...current,
      logs: [entry, ...current.logs].slice(0, 250),
    }));
  }, []);

  const mergeDevices = useCallback((devices: ManagedDeviceSummary[]) => {
    setSnapshot((current) => ({ ...current, devices }));
  }, []);

  const mergeDeviceEndpoints = useCallback((deviceEndpoints: DeviceEndpointConfig[]) => {
    setSnapshot((current) => ({ ...current, deviceEndpoints }));
  }, []);

  const mergeDeviceRoleAssignments = useCallback((deviceRoleAssignments: DeviceRoleAssignment[]) => {
    setSnapshot((current) => ({ ...current, deviceRoleAssignments }));
  }, []);

  const mergeRoleMappings = useCallback((roleMappings: RoleMappingConfig[]) => {
    setSnapshot((current) => ({ ...current, roleMappings }));
  }, []);

  const refreshSnapshot = useCallback(async () => {
    if (!isTauri()) {
      return;
    }

    try {
      const next = await invoke<AppSnapshot>("get_app_state");
      replaceSnapshot(next);
    } catch (error) {
      setRuntimeError(String(error));
    }
  }, [replaceSnapshot]);

  const runAction = useCallback(
    async <T,>(label: string, action: () => Promise<T>) => {
      setBusyAction(label);
      try {
        return await action();
      } finally {
        setBusyAction(null);
      }
    },
    [],
  );

  const saveConfig = useCallback(
    async (config: DcsBiosConnectionConfig) => {
      if (!isTauri()) {
        setSnapshot((current) => ({ ...current, dcsbiosConfig: config }));
        return;
      }

      try {
        const next = await runAction("save-config", () =>
          invoke<AppSnapshot>("update_dcsbios_config", { config }),
        );
        replaceSnapshot(next);
      } catch (error) {
        setRuntimeError(String(error));
      }
    },
    [replaceSnapshot, runAction],
  );

  const startDcsBios = useCallback(async () => {
    if (!isTauri()) {
      return;
    }

    try {
      const next = await runAction("start-dcsbios", () =>
        invoke<AppSnapshot>("start_dcsbios"),
      );
      replaceSnapshot(next);
    } catch (error) {
      setRuntimeError(String(error));
    }
  }, [replaceSnapshot, runAction]);

  const stopDcsBios = useCallback(async () => {
    if (!isTauri()) {
      return;
    }

    try {
      const next = await runAction("stop-dcsbios", () =>
        invoke<AppSnapshot>("stop_dcsbios"),
      );
      replaceSnapshot(next);
    } catch (error) {
      setRuntimeError(String(error));
    }
  }, [replaceSnapshot, runAction]);

  const refreshDevices = useCallback(async () => {
    if (!isTauri()) {
      return;
    }

    try {
      const devices = await runAction("refresh-devices", () =>
        invoke<ManagedDeviceSummary[]>("list_devices"),
      );
      mergeDevices(devices);
    } catch (error) {
      setRuntimeError(String(error));
    }
  }, [mergeDevices, runAction]);

  const saveDeviceEndpoints = useCallback(
    async (deviceEndpoints: DeviceEndpointConfig[]) => {
      if (!isTauri()) {
        setSnapshot((current) => ({ ...current, deviceEndpoints }));
        return;
      }

      try {
        const next = await runAction("save-device-endpoints", () =>
          invoke<AppSnapshot>("save_device_endpoints", { deviceEndpoints }),
        );
        replaceSnapshot(next);
      } catch (error) {
        setRuntimeError(String(error));
      }
    },
    [replaceSnapshot, runAction],
  );

  const refreshSerialPorts = useCallback(async () => {
    if (!isTauri()) {
      return;
    }

    try {
      const ports = await invoke<string[]>("list_serial_ports");
      setSerialPorts(ports);
    } catch (error) {
      setRuntimeError(String(error));
    }
  }, []);

  const saveDeviceRoleAssignments = useCallback(
    async (deviceRoleAssignments: DeviceRoleAssignment[]) => {
      if (!isTauri()) {
        setSnapshot((current) => ({ ...current, deviceRoleAssignments }));
        return;
      }

      try {
        const next = await runAction("save-device-role-assignments", () =>
          invoke<AppSnapshot>("save_device_role_assignments", { deviceRoleAssignments }),
        );
        replaceSnapshot(next);
      } catch (error) {
        setRuntimeError(String(error));
      }
    },
    [replaceSnapshot, runAction],
  );

  const saveRoleMappings = useCallback(
    async (roleMappings: RoleMappingConfig[]) => {
      if (!isTauri()) {
        setSnapshot((current) => ({ ...current, roleMappings }));
        return;
      }

      try {
        const next = await runAction("save-role-mappings", () =>
          invoke<AppSnapshot>("save_role_mappings", { roleMappings }),
        );
        replaceSnapshot(next);
      } catch (error) {
        setRuntimeError(String(error));
      }
    },
    [replaceSnapshot, runAction],
  );

  const sendCommand = useCallback(
    async (request: DcsBiosCommandRequest) => {
      if (!isTauri()) {
        return;
      }

      try {
        await runAction("send-command", () =>
          invoke("send_dcsbios_command", { request }),
        );
      } catch (error) {
        setRuntimeError(String(error));
      }
    },
    [runAction],
  );

  useEffect(() => {
    if (!isTauri()) {
      setRuntimeError("Tauri runtime not detected. The web build only shows the shell UI.");
      return;
    }

    void refreshSnapshot();
    void refreshSerialPorts();

    let disposed = false;
    let unlistenFns: UnlistenFn[] = [];

    const bindListeners = async () => {
      const listeners = await Promise.all([
        listen<DcsBiosStatus>("dcsbios-status-changed", (event) => {
          if (!disposed) {
            mergeStatus(event.payload);
          }
        }),
        listen<ManagerLogEntry>("manager-log", (event) => {
          if (!disposed) {
            mergeLog(event.payload);
          }
        }),
        listen<ManagedDeviceSummary[]>("devices-changed", (event) => {
          if (!disposed) {
            mergeDevices(event.payload);
          }
        }),
        listen<DeviceEndpointConfig[]>("device-endpoints-changed", (event) => {
          if (!disposed) {
            mergeDeviceEndpoints(event.payload);
          }
        }),
        listen<DeviceRoleAssignment[]>("device-role-assignments-changed", (event) => {
          if (!disposed) {
            mergeDeviceRoleAssignments(event.payload);
          }
        }),
        listen<RoleMappingConfig[]>("role-mappings-changed", (event) => {
          if (!disposed) {
            mergeRoleMappings(event.payload);
          }
        }),
      ]);

      unlistenFns = listeners;
    };

    void bindListeners();

    return () => {
      disposed = true;
      for (const unlisten of unlistenFns) {
        void unlisten();
      }
    };
  }, [
    mergeDeviceEndpoints,
    mergeDeviceRoleAssignments,
    mergeDevices,
    mergeLog,
    mergeRoleMappings,
    mergeStatus,
    refreshSerialPorts,
    refreshSnapshot,
  ]);

  return {
    snapshot,
    runtimeError,
    busyAction,
    serialPorts,
    saveConfig,
    startDcsBios,
    stopDcsBios,
    refreshDevices,
    saveDeviceEndpoints,
    saveDeviceRoleAssignments,
    saveRoleMappings,
    refreshSerialPorts,
    sendCommand,
    refreshSnapshot,
  };
}
