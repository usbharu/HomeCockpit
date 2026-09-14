"use client";

import { useEffect, useMemo, useState } from "react";
import { ArrowRightLeft, ChevronRight, Plus, Radio, Trash2 } from "lucide-react";

import {
  deviceRoleLabels,
  getImplementedRoleControls,
  getPhysicalControlCatalog,
  getRoleDefinition,
} from "@/lib/control-catalog";
import type {
  DeviceRoleAssignment,
  EventKind,
  LearnRequest,
  LearnSessionStatus,
  ManagedDeviceSummary,
  RoleDefinition,
} from "@/lib/manager-types";

type MappingSettingsProps = {
  devices: ManagedDeviceSummary[];
  roleDefinitions: RoleDefinition[];
  deviceRoleAssignments: DeviceRoleAssignment[];
  learnSession: LearnSessionStatus;
  busyAction: string | null;
  onSaveDeviceRoleAssignments: (assignments: DeviceRoleAssignment[]) => Promise<void>;
  onStartLearn: (request: LearnRequest) => Promise<void>;
  onCancelLearn: () => Promise<void>;
};

const eventLabels: Record<EventKind, string> = {
  "button-down": "Button Down",
  "button-up": "Button Up",
  "button-pushed": "Button Pushed",
  "encoder-delta": "Encoder Delta",
  "absolute-changed": "Absolute Changed",
  "toggle-on": "Toggle On",
  "toggle-off": "Toggle Off",
};

export function MappingSettings({
  devices,
  roleDefinitions,
  deviceRoleAssignments,
  learnSession,
  busyAction,
  onSaveDeviceRoleAssignments,
  onStartLearn,
  onCancelLearn,
}: MappingSettingsProps) {
  const [selectedRoleId, setSelectedRoleId] = useState(roleDefinitions[0]?.roleId ?? "");
  const [selectedDeviceId, setSelectedDeviceId] = useState("");
  const [selectedLogicalControlId, setSelectedLogicalControlId] = useState("");
  const [selectedPhysicalControlId, setSelectedPhysicalControlId] = useState("");
  const [learnMode, setLearnMode] = useState<"append" | "replace">("append");
  const [learnDeviceId, setLearnDeviceId] = useState("");
  const [learnExpectedEventKind, setLearnExpectedEventKind] = useState<EventKind | "">("");

  const roleDefinition = useMemo(
    () => getRoleDefinition(selectedRoleId, roleDefinitions),
    [roleDefinitions, selectedRoleId],
  );
  const roleAssignments = useMemo(
    () => deviceRoleAssignments.filter((entry) => entry.roleId === selectedRoleId),
    [deviceRoleAssignments, selectedRoleId],
  );
  const roleControlCapacity = roleAssignments.reduce((maximum, assignment) => {
    const device = devices.find((entry) => entry.deviceId === assignment.deviceId);
    return Math.max(maximum, device?.controls ?? 0);
  }, 0);
  const roleControls = useMemo(
    () => getImplementedRoleControls(roleDefinition, roleControlCapacity),
    [roleControlCapacity, roleDefinition],
  );
  const learnDevices = useMemo(
    () =>
      devices.filter(
        (device): device is ManagedDeviceSummary & { deviceId: string } =>
          device.deviceId !== null,
      ),
    [devices],
  );
  const selectedControl = roleControls.find(
    (control) => control.logicalControlId === selectedLogicalControlId,
  );
  const selectedDevice = devices.find((device) => device.deviceId === selectedDeviceId) ?? null;
  const selectedPhysicalControls = useMemo(
    () =>
      getPhysicalControlCatalog(
        selectedDevice?.deviceKindId ?? null,
        selectedDevice?.controls ?? null,
      ),
    [selectedDevice?.controls, selectedDevice?.deviceKindId],
  );

  useEffect(() => {
    if (roleDefinitions.some((definition) => definition.roleId === selectedRoleId)) {
      return;
    }
    setSelectedRoleId(roleDefinitions[0]?.roleId ?? "");
  }, [roleDefinitions, selectedRoleId]);

  useEffect(() => {
    const firstControl = roleControls[0]?.logicalControlId ?? "";
    setSelectedLogicalControlId((current) =>
      roleControls.some((control) => control.logicalControlId === current) ? current : firstControl,
    );
    setSelectedDeviceId((current) =>
      roleAssignments.some((assignment) => assignment.deviceId === current)
        ? current
        : roleAssignments[0]?.deviceId ?? "",
    );
  }, [roleAssignments, roleControls]);

  useEffect(() => {
    setSelectedPhysicalControlId((current) => {
      if (selectedPhysicalControls.some((control) => String(control.physicalControlId) === current)) {
        return current;
      }
      return selectedPhysicalControls[0] ? String(selectedPhysicalControls[0].physicalControlId) : "";
    });
  }, [selectedPhysicalControls]);

  const updateAssignments = async (
    updater: (assignments: DeviceRoleAssignment[]) => DeviceRoleAssignment[],
  ) => {
    await onSaveDeviceRoleAssignments(updater(deviceRoleAssignments));
  };

  const addManualBinding = async () => {
    const physicalControlId = Number(selectedPhysicalControlId);
    if (!selectedDeviceId || !selectedLogicalControlId || !Number.isInteger(physicalControlId)) {
      return;
    }

    await updateAssignments((assignments) =>
      assignments.map((assignment) => {
        if (assignment.deviceId !== selectedDeviceId || assignment.roleId !== selectedRoleId) {
          return assignment;
        }
        if (
          assignment.bindings.some(
            (binding) =>
              binding.physicalControlId === physicalControlId &&
              binding.logicalControlId === selectedLogicalControlId,
          )
        ) {
          return assignment;
        }
        return {
          ...assignment,
          bindings: [
            ...assignment.bindings,
            { physicalControlId, logicalControlId: selectedLogicalControlId },
          ],
        };
      }),
    );
  };

  const removeBinding = async (
    deviceId: string,
    logicalControlId: string,
    physicalControlId: number,
  ) => {
    await updateAssignments((assignments) =>
      assignments.map((assignment) =>
        assignment.deviceId === deviceId && assignment.roleId === selectedRoleId
          ? {
              ...assignment,
              bindings: assignment.bindings.filter(
                (binding) =>
                  !(
                    binding.logicalControlId === logicalControlId &&
                    binding.physicalControlId === physicalControlId
                  ),
              ),
            }
          : assignment,
      ),
    );
  };

  const startLearn = async () => {
    if (!selectedLogicalControlId || learnSession.active) {
      return;
    }
    await onStartLearn({
      roleId: selectedRoleId,
      logicalControlId: selectedLogicalControlId,
      targetDeviceId: learnDeviceId || null,
      expectedEventKind: learnExpectedEventKind || null,
      mode: learnMode,
      timeoutMs: 10_000,
    });
  };

  const labelForPhysicalControl = (device: ManagedDeviceSummary, physicalControlId: number) => {
    const control = getPhysicalControlCatalog(device.deviceKindId, device.controls).find(
      (entry) => entry.physicalControlId === physicalControlId,
    );
    return control ? `${control.label} (${control.description})` : `Physical ${physicalControlId}`;
  };

  const implementedControlCount = (roleId: string) =>
    deviceRoleAssignments
      .filter((assignment) => assignment.roleId === roleId)
      .reduce((maximum, assignment) => {
        const device = devices.find((entry) => entry.deviceId === assignment.deviceId);
        return Math.max(maximum, device?.controls ?? 0);
      }, 0);

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="mx-auto flex max-w-7xl flex-col gap-6">
        <div className="grid gap-6 xl:grid-cols-[320px_minmax(0,1fr)]">
          <aside className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
            <p className="text-sm font-medium text-gray-500">論理 Role</p>
            <h3 className="mt-1 text-xl font-semibold text-gray-900">Role 一覧</h3>
            <div className="mt-5 space-y-3">
              {roleDefinitions.map((definition) => {
                const isSelected = definition.roleId === selectedRoleId;
                const assignmentCount = deviceRoleAssignments.filter(
                  (assignment) => assignment.roleId === definition.roleId,
                ).length;
                return (
                  <button
                    key={definition.roleId}
                    type="button"
                    onClick={() => setSelectedRoleId(definition.roleId)}
                    className={`w-full rounded-lg border p-4 text-left transition ${
                      isSelected
                        ? "border-blue-200 bg-blue-50"
                        : "border-gray-200 bg-white hover:border-gray-300 hover:bg-gray-50"
                    }`}
                  >
                    <div className="flex items-start gap-3">
                      <div className="mt-0.5 flex h-11 w-11 items-center justify-center rounded-lg bg-gray-100 text-blue-600">
                        <ArrowRightLeft size={18} />
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center justify-between gap-3">
                          <p className="font-semibold text-gray-900">
                            {deviceRoleLabels[definition.roleId] ?? definition.roleId}
                          </p>
                          <ChevronRight size={18} className={isSelected ? "text-blue-600" : "text-gray-400"} />
                        </div>
                        <p className="mt-1 text-sm text-gray-500">{definition.roleId}</p>
                        <div className="mt-3 flex items-center justify-between">
                          <span className="rounded-full border border-gray-200 bg-gray-50 px-2.5 py-1 text-xs text-gray-600">
                            {assignmentCount} device(s)
                          </span>
                          <span className="text-xs text-gray-400">
                            {implementedControlCount(definition.roleId)} implemented controls
                          </span>
                        </div>
                      </div>
                    </div>
                  </button>
                );
              })}
            </div>
          </aside>

          <section className="space-y-6">
            {!roleDefinition ? (
              <div className="rounded-lg border border-dashed border-gray-300 bg-gray-50 p-8 text-sm text-gray-500">
                Role 定義がありません。
              </div>
            ) : (
              <>
                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                    <div>
                      <p className="text-sm font-medium text-gray-500">Role 論理インターフェース</p>
                      <h3 className="mt-1 text-3xl font-semibold tracking-tight text-gray-900">
                        {deviceRoleLabels[selectedRoleId] ?? selectedRoleId}
                      </h3>
                      <p className="mt-2 text-sm text-gray-500">
                        論理コントロールはゲームや物理デバイスを知りません。下の全デバイス結線へ fan-out します。
                      </p>
                    </div>
                    <div className="rounded-full border border-gray-200 bg-gray-50 px-4 py-2 text-sm text-gray-700">
                      {roleAssignments.length} device(s) / {roleControls.length} logical control(s)
                    </div>
                  </div>
                  <p className="mt-4 rounded-md border border-blue-100 bg-blue-50 px-4 py-3 text-sm text-blue-800">
                    Adapterは現在の航空機を自動判定します。未知の航空機の定義は「Adapter設定」タブで作成してください。
                  </p>
                </section>

                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <div className="flex items-center justify-between gap-4">
                    <div>
                      <h3 className="text-xl font-semibold text-gray-900">物理 → 論理結線</h3>
                      <p className="mt-1 text-sm text-gray-500">
                        1つの物理入力を複数Role／論理コントロールへ登録できます。重複は警告だけで拒否しません。
                      </p>
                    </div>
                    <div className="rounded-full border border-gray-200 bg-gray-100 px-4 py-2 text-sm text-gray-700">
                      {roleAssignments.reduce((count, assignment) => count + assignment.bindings.length, 0)} binding(s)
                    </div>
                  </div>

                  <div className="mt-5 grid gap-4 rounded-lg border border-blue-100 bg-blue-50/50 p-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_120px]">
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Device</span>
                      <select
                        value={selectedDeviceId}
                        onChange={(event) => setSelectedDeviceId(event.target.value)}
                        disabled={roleAssignments.length === 0}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="">デバイスを選択</option>
                        {roleAssignments.map((assignment) => (
                          <option key={assignment.deviceId} value={assignment.deviceId}>
                            {devices.find((device) => device.deviceId === assignment.deviceId)?.displayName ?? assignment.deviceId}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Logical Control</span>
                      <select
                        value={selectedLogicalControlId}
                        onChange={(event) => setSelectedLogicalControlId(event.target.value)}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500"
                      >
                        {roleControls.map((control) => (
                          <option key={control.logicalControlId} value={control.logicalControlId}>
                            {control.label} ({control.logicalControlId})
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Physical Control</span>
                      <select
                        value={selectedPhysicalControlId}
                        onChange={(event) => setSelectedPhysicalControlId(event.target.value)}
                        disabled={selectedPhysicalControls.length === 0}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="">物理コントロールを選択</option>
                        {selectedPhysicalControls.map((control) => (
                          <option key={control.physicalControlId} value={control.physicalControlId}>
                            {control.label} ({control.description})
                          </option>
                        ))}
                      </select>
                    </label>
                    <button
                      type="button"
                      onClick={() => void addManualBinding()}
                      disabled={busyAction !== null || !selectedDeviceId || selectedPhysicalControls.length === 0}
                      className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md bg-blue-600 px-4 text-sm font-medium text-white transition hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      <Plus size={16} />
                      追加
                    </button>
                  </div>

                  {roleAssignments.length === 0 ? (
                    <div className="mt-5 rounded-lg border border-dashed border-gray-300 bg-gray-50 p-6 text-sm text-gray-500">
                      デバイス設定タブで、このRoleに1台以上のデバイスを追加してください。
                    </div>
                  ) : (
                    <div className="mt-5 space-y-3">
                      {roleControls.map((control) => {
                        const bindings = roleAssignments.flatMap((assignment) => {
                          const device = devices.find((entry) => entry.deviceId === assignment.deviceId);
                          return assignment.bindings
                            .filter((binding) => binding.logicalControlId === control.logicalControlId)
                            .map((binding) => ({ assignment, device, binding }));
                        });
                        return (
                          <div key={control.logicalControlId} className="rounded-lg border border-gray-200 bg-gray-50 p-4">
                            <div className="flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-between">
                              <div>
                                <p className="font-medium text-gray-900">{control.label}</p>
                                <p className="text-xs text-gray-500">
                                  {control.logicalControlId} · {control.supportedEvents.map((event) => eventLabels[event]).join(", ")}
                                </p>
                              </div>
                              {bindings.length > 1 && (
                                <span className="inline-flex items-center gap-2 rounded-full border border-amber-200 bg-amber-50 px-3 py-1 text-xs text-amber-800">
                                  <Radio size={13} /> 複数の経路に接続されています
                                </span>
                              )}
                            </div>
                            <div className="mt-3 flex flex-wrap gap-2">
                              {bindings.length === 0 ? (
                                <span className="text-xs text-gray-400">未結線</span>
                              ) : (
                                bindings.map(({ assignment, device, binding }) => (
                                  <span
                                    key={`${assignment.deviceId}:${binding.physicalControlId}:${binding.logicalControlId}`}
                                    className="inline-flex items-center gap-2 rounded-full border border-gray-200 bg-white px-3 py-1.5 text-xs text-gray-700"
                                  >
                                    {device?.displayName ?? assignment.deviceId} · {device ? labelForPhysicalControl(device, binding.physicalControlId) : `Physical ${binding.physicalControlId}`}
                                    <button
                                      type="button"
                                      onClick={() => void removeBinding(assignment.deviceId, binding.logicalControlId, binding.physicalControlId)}
                                      disabled={busyAction !== null}
                                      className="text-gray-400 hover:text-red-600 disabled:opacity-50"
                                      aria-label="物理結線を削除"
                                    >
                                      <Trash2 size={13} />
                                    </button>
                                  </span>
                                ))
                              )}
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </section>

                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                    <div>
                      <h3 className="text-xl font-semibold text-gray-900">コントロールを学習</h3>
                      <p className="mt-1 text-sm text-gray-500">
                        指定した論理コントロールに対して、次に受信したHCP ControlEventを登録します。学習中のイベントはAdapterへ送信しません。
                      </p>
                    </div>
                    {learnSession.active && (
                      <button
                        type="button"
                        onClick={() => void onCancelLearn()}
                        disabled={busyAction !== null}
                        className="rounded-md border border-red-200 bg-red-50 px-4 py-2 text-sm font-medium text-red-700 hover:bg-red-100 disabled:opacity-50"
                      >
                        学習をキャンセル
                      </button>
                    )}
                  </div>
                  <div className="mt-5 grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_180px_180px_140px]">
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>Logical Control</span>
                      <select
                        value={selectedLogicalControlId}
                        onChange={(event) => setSelectedLogicalControlId(event.target.value)}
                        disabled={learnSession.active}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        {roleControls.map((control) => (
                          <option key={control.logicalControlId} value={control.logicalControlId}>
                            {control.label}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>対象Device（任意）</span>
                      <select
                        value={learnDeviceId}
                        onChange={(event) => setLearnDeviceId(event.target.value)}
                        disabled={learnSession.active}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="">最初に受信したDevice</option>
                        {learnDevices.map((device) => (
                          <option key={device.deviceId} value={device.deviceId}>
                            {device.displayName} · {device.deviceId}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>イベント種別（任意）</span>
                      <select
                        value={learnExpectedEventKind}
                        onChange={(event) => setLearnExpectedEventKind(event.target.value as EventKind | "")}
                        disabled={learnSession.active}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="">任意</option>
                        {(selectedControl?.supportedEvents ?? []).map((event) => (
                          <option key={event} value={event}>{eventLabels[event]}</option>
                        ))}
                      </select>
                    </label>
                    <label className="space-y-2 text-sm text-gray-700">
                      <span>既存結線</span>
                      <select
                        value={learnMode}
                        onChange={(event) => setLearnMode(event.target.value as "append" | "replace")}
                        disabled={learnSession.active}
                        className="w-full rounded-md border border-gray-300 bg-white px-3 py-2 outline-none transition focus:border-blue-500 disabled:bg-gray-100"
                      >
                        <option value="append">追加</option>
                        <option value="replace">上書き</option>
                      </select>
                    </label>
                    <button
                      type="button"
                      onClick={() => void startLearn()}
                      disabled={busyAction !== null || learnSession.active || !selectedLogicalControlId}
                      className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md bg-indigo-600 px-4 text-sm font-medium text-white hover:bg-indigo-700 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      <Radio size={16} />
                      {learnSession.active ? "入力待ち…" : "学習開始"}
                    </button>
                  </div>
                  {learnSession.active && (
                    <p className="mt-4 rounded-md border border-indigo-100 bg-indigo-50 px-4 py-3 text-sm text-indigo-800">
                      {learnSession.targetDeviceId ? `Device ${learnSession.targetDeviceId}` : "最初に受信したDevice"}の入力を待っています。{learnSession.timeoutMs / 1000}秒でタイムアウトします。
                    </p>
                  )}
                </section>
              </>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}

export default MappingSettings;
