"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowRightLeft,
  Check,
  ChevronRight,
  Play,
  Plus,
  Radio,
  RotateCcw,
  Trash2,
} from "lucide-react";

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
  RoleInputTriggerRequest,
} from "@/lib/manager-types";

type MappingSettingsProps = {
  devices: ManagedDeviceSummary[];
  roleDefinitions: RoleDefinition[];
  deviceRoleAssignments: DeviceRoleAssignment[];
  learnSession: LearnSessionStatus;
  busyAction: string | null;
  onSaveDeviceRoleAssignments: (assignments: DeviceRoleAssignment[]) => Promise<void>;
  onTriggerRoleInput: (request: RoleInputTriggerRequest) => Promise<number>;
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

type ContinuousLearnPhase = "arming" | "waiting" | "canceling" | "paused" | "completed";

type ContinuousLearnState = {
  runId: number;
  roleId: string;
  targetDeviceId: string;
  logicalControlIds: string[];
  currentIndex: number;
  phase: ContinuousLearnPhase;
  notice: string | null;
};

export function MappingSettings({
  devices,
  roleDefinitions,
  deviceRoleAssignments,
  learnSession,
  busyAction,
  onSaveDeviceRoleAssignments,
  onTriggerRoleInput,
  onStartLearn,
  onCancelLearn,
}: MappingSettingsProps) {
  const [selectedRoleId, setSelectedRoleId] = useState(roleDefinitions[0]?.roleId ?? "");
  const [bindingDrafts, setBindingDrafts] = useState<Record<string, { deviceId: string; physicalControlId: string }>>({});
  const [triggeringRoleAction, setTriggeringRoleAction] = useState<string | null>(null);
  const [roleActionNotice, setRoleActionNotice] = useState<{ controlId: string; message: string } | null>(null);
  const [continuousLearnDeviceId, setContinuousLearnDeviceId] = useState("");
  const [continuousLearn, setContinuousLearn] = useState<ContinuousLearnState | null>(null);
  const [learnNotice, setLearnNotice] = useState<string | null>(null);
  const continuousLearnRunId = useRef(0);
  const armingRequestKey = useRef<string | null>(null);

  const roleDefinition = useMemo(
    () => getRoleDefinition(selectedRoleId, roleDefinitions),
    [roleDefinitions, selectedRoleId],
  );
  const roleAssignments = useMemo(
    () => deviceRoleAssignments.filter((entry) => entry.roleId === selectedRoleId),
    [deviceRoleAssignments, selectedRoleId],
  );
  const hasRoleAssignments = roleAssignments.length > 0;
  const roleControlCapacity = roleAssignments.reduce((maximum, assignment) => {
    const device = devices.find((entry) => entry.deviceId === assignment.deviceId);
    return Math.max(maximum, device?.controls ?? 0);
  }, 0);
  const roleControls = useMemo(
    () => getImplementedRoleControls(roleDefinition, roleControlCapacity),
    [roleControlCapacity, roleDefinition],
  );

  const continuousLearnAssignment = roleAssignments.find(
    (assignment) => assignment.deviceId === continuousLearnDeviceId,
  );
  const continuousLearnCandidates = useMemo(() => {
    const boundLogicalControlIds = new Set(
      continuousLearnAssignment?.bindings.map((binding) => binding.logicalControlId) ?? [],
    );
    const seenLogicalControlIds = new Set<string>();

    return roleControls.filter((control) => {
      if (seenLogicalControlIds.has(control.logicalControlId)) {
        return false;
      }
      seenLogicalControlIds.add(control.logicalControlId);
      return !boundLogicalControlIds.has(control.logicalControlId);
    });
  }, [continuousLearnAssignment, roleControls]);

  const continuousLearnTargetDevice = devices.find(
    (device) => device.deviceId === (continuousLearn?.targetDeviceId ?? continuousLearnDeviceId),
  );
  const continuousLearnQueueControls = continuousLearn
    ? continuousLearn.logicalControlIds.map((logicalControlId) =>
        roleControls.find((control) => control.logicalControlId === logicalControlId),
      )
    : [];
  const currentContinuousLearnControl = continuousLearn
    ? continuousLearnQueueControls[continuousLearn.currentIndex] ?? null
    : null;
  const continuousLearnRemainingCount = continuousLearn
    ? Math.max(continuousLearn.logicalControlIds.length - continuousLearn.currentIndex, 0)
    : 0;
  const continuousLearnCompletedCount = continuousLearn
    ? Math.min(continuousLearn.currentIndex, continuousLearn.logicalControlIds.length)
    : 0;

  useEffect(() => {
    if (roleDefinitions.some((definition) => definition.roleId === selectedRoleId)) {
      return;
    }
    setSelectedRoleId(roleDefinitions[0]?.roleId ?? "");
  }, [roleDefinitions, selectedRoleId]);

  useEffect(() => {
    if (continuousLearn) {
      return;
    }
    if (roleAssignments.some((assignment) => assignment.deviceId === continuousLearnDeviceId)) {
      return;
    }
    setContinuousLearnDeviceId(roleAssignments[0]?.deviceId ?? "");
  }, [continuousLearn, continuousLearnDeviceId, roleAssignments]);

  const updateAssignments = async (
    updater: (assignments: DeviceRoleAssignment[]) => DeviceRoleAssignment[],
  ) => {
    await onSaveDeviceRoleAssignments(updater(deviceRoleAssignments));
  };

  const addManualBinding = async (
    logicalControlId: string,
    deviceId: string,
    physicalControlIdValue: string,
  ) => {
    const physicalControlId = Number(physicalControlIdValue);
    if (!deviceId || !physicalControlIdValue || !Number.isInteger(physicalControlId)) {
      return;
    }

    await updateAssignments((assignments) =>
      assignments.map((assignment) => {
        if (assignment.deviceId !== deviceId || assignment.roleId !== selectedRoleId) {
          return assignment;
        }
        if (
          assignment.bindings.some(
            (binding) =>
              binding.physicalControlId === physicalControlId &&
              binding.logicalControlId === logicalControlId,
          )
        ) {
          return assignment;
        }
        return {
          ...assignment,
          bindings: [
            ...assignment.bindings,
            { physicalControlId, logicalControlId },
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

  const triggerRoleAction = async (logicalControlId: string, eventKind: EventKind) => {
    const actionKey = `${logicalControlId}:${eventKind}`;
    setTriggeringRoleAction(actionKey);
    setRoleActionNotice(null);
    try {
      const actionCount = await onTriggerRoleInput({
        roleId: selectedRoleId,
        logicalControlId,
        eventKind,
      });
      setRoleActionNotice({
        controlId: logicalControlId,
        message: `${eventLabels[eventKind]}を実行しました（${actionCount} action）。`,
      });
    } catch (error) {
      setRoleActionNotice({
        controlId: logicalControlId,
        message: `Role操作に失敗しました: ${String(error)}`,
      });
    } finally {
      setTriggeringRoleAction(null);
    }
  };

  const startSingleLearn = async (logicalControlId: string, targetDeviceId: string) => {
    if (
      !targetDeviceId ||
      learnSession.active ||
      (continuousLearn !== null && continuousLearn.phase !== "paused")
    ) {
      return;
    }
    setLearnNotice(null);
    try {
      await onStartLearn({
        roleId: selectedRoleId,
        logicalControlId,
        targetDeviceId: targetDeviceId || null,
        expectedEventKind: null,
        mode: "append",
        timeoutMs: 10_000,
      });
    } catch (error) {
      setLearnNotice(`学習開始に失敗しました: ${String(error)}`);
    }
  };

  const cancelSingleLearn = async () => {
    try {
      await onCancelLearn();
    } catch (error) {
      setLearnNotice(`学習取消に失敗しました: ${String(error)}`);
    }
  };

  const beginContinuousLearn = () => {
    if (
      busyAction !== null ||
      learnSession.active ||
      continuousLearn !== null ||
      !continuousLearnDeviceId ||
      continuousLearnCandidates.length === 0
    ) {
      return;
    }

    continuousLearnRunId.current += 1;
    armingRequestKey.current = null;
    setLearnNotice(null);
    setContinuousLearn({
      runId: continuousLearnRunId.current,
      roleId: selectedRoleId,
      targetDeviceId: continuousLearnDeviceId,
      logicalControlIds: continuousLearnCandidates.map((control) => control.logicalControlId),
      currentIndex: 0,
      phase: "arming",
      notice: null,
    });
  };

  const resumeContinuousLearn = () => {
    if (!continuousLearn || continuousLearn.phase !== "paused" || learnSession.active) {
      return;
    }

    armingRequestKey.current = null;
    setLearnNotice(null);
    setContinuousLearn((current) =>
      current
        ? {
            ...current,
            phase: "arming",
            notice: null,
          }
        : current,
    );
  };

  const discardContinuousLearn = () => {
    if (
      continuousLearn &&
      continuousLearn.phase !== "paused" &&
      continuousLearn.phase !== "completed"
    ) {
      return;
    }

    continuousLearnRunId.current += 1;
    armingRequestKey.current = null;
    setContinuousLearn(null);
    setLearnNotice(null);
  };

  const cancelContinuousLearn = async () => {
    if (!continuousLearn || continuousLearn.phase !== "waiting") {
      return;
    }

    const runId = continuousLearn.runId;
    armingRequestKey.current = null;
    setContinuousLearn((current) =>
      current && current.runId === runId
        ? {
            ...current,
            phase: "canceling",
            notice: null,
          }
        : current,
    );

    try {
      await onCancelLearn();
      setContinuousLearn((current) =>
        current && current.runId === runId
          ? {
              ...current,
              phase: "paused",
              notice: "連続学習を停止しました。現在の項目から再開できます。",
            }
          : current,
      );
    } catch (error) {
      setContinuousLearn((current) =>
        current && current.runId === runId
          ? {
              ...current,
              phase: "waiting",
              notice: `連続学習の取消に失敗しました: ${String(error)}`,
            }
          : current,
      );
    }
  };

  const handleLearnCancel = () => {
    if (continuousLearn?.phase === "waiting") {
      return cancelContinuousLearn();
    }
    return cancelSingleLearn();
  };

  useEffect(() => {
    const batch = continuousLearn;
    if (!batch || batch.phase !== "arming") {
      return;
    }

    const logicalControlId = batch.logicalControlIds[batch.currentIndex];
    if (!logicalControlId) {
      setContinuousLearn((current) =>
        current && current.runId === batch.runId
          ? {
              ...current,
              currentIndex: current.logicalControlIds.length,
              phase: "completed",
              notice: "連続学習が完了しました。",
            }
          : current,
      );
      return;
    }

    const requestKey = `${batch.runId}:${batch.currentIndex}:${logicalControlId}`;
    if (armingRequestKey.current === requestKey) {
      return;
    }
    armingRequestKey.current = requestKey;

    const control = roleControls.find((entry) => entry.logicalControlId === logicalControlId);
    const expectedEventKind = control?.supportedEvents.includes("button-pushed")
      ? "button-pushed"
      : null;
    void onStartLearn({
      roleId: batch.roleId,
      logicalControlId,
      targetDeviceId: batch.targetDeviceId,
      expectedEventKind,
      mode: "append",
      timeoutMs: 10_000,
    })
      .then(() => {
        setContinuousLearn((current) =>
          current &&
          current.runId === batch.runId &&
          current.currentIndex === batch.currentIndex &&
          current.phase === "arming"
            ? {
                ...current,
                phase: "waiting",
                notice: null,
              }
            : current,
        );
      })
      .catch((error) => {
        setContinuousLearn((current) =>
          current &&
          current.runId === batch.runId &&
          current.currentIndex === batch.currentIndex &&
          current.phase === "arming"
            ? {
                ...current,
                phase: "paused",
                notice: `学習開始に失敗しました: ${String(error)}`,
              }
            : current,
        );
      });
  }, [continuousLearn, onStartLearn, roleControls]);

  useEffect(() => {
    const batch = continuousLearn;
    if (!batch || batch.phase !== "waiting") {
      return;
    }

    const logicalControlId = batch.logicalControlIds[batch.currentIndex];
    if (!logicalControlId || learnSession.active) {
      return;
    }

    const capturedSuccessfully =
      learnSession.roleId === batch.roleId &&
      learnSession.logicalControlId === logicalControlId &&
      learnSession.targetDeviceId === batch.targetDeviceId &&
      learnSession.capturedDeviceId === batch.targetDeviceId &&
      learnSession.capturedPhysicalControlId !== null;

    if (capturedSuccessfully) {
      setContinuousLearn((current) => {
        if (
          !current ||
          current.runId !== batch.runId ||
          current.currentIndex !== batch.currentIndex ||
          current.phase !== "waiting"
        ) {
          return current;
        }

        const nextIndex = current.currentIndex + 1;
        if (nextIndex >= current.logicalControlIds.length) {
          return {
            ...current,
            currentIndex: current.logicalControlIds.length,
            phase: "completed",
            notice: "連続学習が完了しました。",
          };
        }

        return {
          ...current,
          currentIndex: nextIndex,
          phase: "arming",
          notice: null,
        };
      });
      return;
    }

    setContinuousLearn((current) =>
      current && current.runId === batch.runId && current.phase === "waiting"
        ? {
            ...current,
            phase: "paused",
            notice: "学習がキャンセルされたかタイムアウトしました。現在の項目から再開できます。",
          }
        : current,
    );
  }, [continuousLearn, learnSession]);

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
                    disabled={continuousLearn !== null}
                    className={`w-full rounded-lg border p-4 text-left transition ${
                      isSelected
                        ? "border-blue-200 bg-blue-50"
                        : "border-gray-200 bg-white hover:border-gray-300 hover:bg-gray-50"
                    } disabled:cursor-not-allowed disabled:opacity-60`}
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
                    この画面は物理Control IDとRoleのLogical Controlだけを結線します。ゲームやAdapterの設定は「Adapter設定」タブで管理します。
                  </p>
                </section>

                <section className="rounded-lg border border-indigo-200 bg-indigo-50/40 p-6 shadow-sm">
                  <div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                    <div>
                      <p className="text-sm font-medium text-indigo-700">一括登録</p>
                      <h3 className="mt-1 text-xl font-semibold text-gray-900">未登録を連続学習</h3>
                      <p className="mt-1 text-sm text-gray-600">
                        対象 Device の未結線 logical control を、画面の定義順に1件ずつ学習します。
                      </p>
                    </div>
                    <div className="rounded-full border border-indigo-200 bg-white px-4 py-2 text-sm text-indigo-800">
                      {continuousLearn
                        ? `${continuousLearnCompletedCount}/${continuousLearn.logicalControlIds.length} 完了`
                        : `${continuousLearnCandidates.length} 件残り`}
                    </div>
                  </div>

                  {!continuousLearn ? (
                    roleAssignments.length === 0 ? (
                      <p className="mt-5 rounded-md border border-dashed border-indigo-200 bg-white/70 p-4 text-sm text-gray-600">
                        この Role に割り当てられた Device がありません。
                      </p>
                    ) : (
                      <>
                        <div className="mt-5 grid gap-3 rounded-md border border-indigo-100 bg-white p-4 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
                          <label className="space-y-1 text-xs text-gray-600">
                            <span>対象 Device</span>
                            <select
                              value={continuousLearnDeviceId}
                              onChange={(event) => setContinuousLearnDeviceId(event.target.value)}
                              disabled={busyAction !== null || learnSession.active}
                              className="w-full rounded-md border border-gray-300 bg-white px-2 py-2 text-xs disabled:bg-gray-100"
                            >
                              {roleAssignments.map((assignment) => (
                                <option key={assignment.deviceId} value={assignment.deviceId}>
                                  {devices.find((device) => device.deviceId === assignment.deviceId)?.displayName ?? assignment.deviceId}
                                </option>
                              ))}
                            </select>
                          </label>
                          <button
                            type="button"
                            onClick={beginContinuousLearn}
                            disabled={
                              busyAction !== null ||
                              learnSession.active ||
                              !continuousLearnDeviceId ||
                              continuousLearnCandidates.length === 0
                            }
                            className="inline-flex h-9 items-center justify-center gap-1.5 rounded-md bg-indigo-600 px-3 text-xs font-medium text-white disabled:cursor-not-allowed disabled:opacity-50"
                          >
                            <Radio size={14} /> 連続学習開始
                          </button>
                        </div>

                        {continuousLearnCandidates.length === 0 ? (
                          <p className="mt-3 rounded-md border border-green-200 bg-green-50 p-4 text-sm text-green-800">
                            この Device には未登録の logical control がありません。
                          </p>
                        ) : (
                          <div className="mt-4 rounded-md border border-indigo-100 bg-white p-4">
                            <p className="text-xs font-medium text-gray-500">登録対象の順序（{continuousLearnCandidates.length}件）</p>
                            <ol className="mt-3 grid gap-2 sm:grid-cols-2">
                              {continuousLearnCandidates.map((control, index) => (
                                <li key={control.logicalControlId} className="flex items-center gap-2 text-sm text-gray-700">
                                  <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-indigo-100 text-xs font-semibold text-indigo-700">
                                    {index + 1}
                                  </span>
                                  <span>{control.label}</span>
                                  <span className="text-xs text-gray-400">{control.logicalControlId}</span>
                                </li>
                              ))}
                            </ol>
                          </div>
                        )}
                      </>
                    )
                  ) : (
                    <div className="mt-5 rounded-md border border-indigo-100 bg-white p-4">
                      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                        <div className="text-sm text-gray-700">
                          <p>
                            対象: <span className="font-medium">{deviceRoleLabels[continuousLearn.roleId] ?? continuousLearn.roleId}</span>
                            {" / "}
                            <span className="font-medium">
                              {continuousLearnTargetDevice?.displayName ?? continuousLearn.targetDeviceId}
                            </span>
                          </p>
                          <p className="mt-1 text-xs text-gray-500">
                            {continuousLearn.phase === "arming"
                              ? "次の学習を開始しています…"
                              : continuousLearn.phase === "waiting"
                                ? "物理入力を待っています。"
                                : continuousLearn.phase === "canceling"
                                  ? "学習を停止しています…"
                                  : continuousLearn.phase === "paused"
                                    ? "停止中。現在の項目から再開できます。"
                                    : "すべての学習が完了しました。"}
                          </p>
                        </div>
                        <div className="flex flex-wrap gap-2 text-xs">
                          <span className="rounded-full border border-gray-200 bg-gray-50 px-3 py-1.5 text-gray-700">
                            現在: {currentContinuousLearnControl?.label ?? "完了"}
                          </span>
                          <span className="rounded-full border border-gray-200 bg-gray-50 px-3 py-1.5 text-gray-700">
                            残り: {continuousLearnRemainingCount}件
                          </span>
                        </div>
                      </div>

                      <ol className="mt-4 grid gap-2 sm:grid-cols-2">
                        {continuousLearn.logicalControlIds.map((logicalControlId, index) => {
                          const isCompleted = index < continuousLearn.currentIndex;
                          const isCurrent = index === continuousLearn.currentIndex && !isCompleted;
                          const control = continuousLearnQueueControls[index];
                          return (
                            <li
                              key={`${continuousLearn.runId}:${logicalControlId}`}
                              className={`flex items-center gap-2 rounded-md border px-3 py-2 text-sm ${
                                isCompleted
                                  ? "border-green-200 bg-green-50 text-green-800"
                                  : isCurrent
                                    ? "border-indigo-200 bg-indigo-50 text-indigo-900"
                                    : "border-gray-200 bg-gray-50 text-gray-600"
                              }`}
                            >
                              <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-white text-xs font-semibold">
                                {isCompleted ? <Check size={14} /> : index + 1}
                              </span>
                              <span className="min-w-0 flex-1 truncate">{control?.label ?? logicalControlId}</span>
                              <span className="text-xs opacity-70">
                                {isCompleted ? "完了" : isCurrent ? "現在" : "待機"}
                              </span>
                            </li>
                          );
                        })}
                      </ol>

                      {continuousLearn.notice && (
                        <p className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800" aria-live="polite">
                          {continuousLearn.notice}
                        </p>
                      )}

                      <div className="mt-4 flex flex-wrap gap-2">
                        {continuousLearn.phase === "waiting" && (
                          <button
                            type="button"
                            onClick={() => void cancelContinuousLearn()}
                            disabled={busyAction !== null}
                            className="inline-flex h-9 items-center justify-center gap-1.5 rounded-md border border-red-200 bg-red-50 px-3 text-xs font-medium text-red-700 disabled:opacity-50"
                          >
                            学習取消
                          </button>
                        )}
                        {continuousLearn.phase === "canceling" && (
                          <button
                            type="button"
                            disabled
                            className="inline-flex h-9 items-center justify-center rounded-md border border-gray-200 bg-gray-100 px-3 text-xs font-medium text-gray-500"
                          >
                            取消中…
                          </button>
                        )}
                        {continuousLearn.phase === "paused" && (
                          <>
                            <button
                              type="button"
                              onClick={resumeContinuousLearn}
                              disabled={busyAction !== null || learnSession.active}
                              className="inline-flex h-9 items-center justify-center gap-1.5 rounded-md bg-indigo-600 px-3 text-xs font-medium text-white disabled:cursor-not-allowed disabled:opacity-50"
                            >
                              <RotateCcw size={14} /> 再開
                            </button>
                            <button
                              type="button"
                              onClick={discardContinuousLearn}
                              disabled={busyAction !== null}
                              className="inline-flex h-9 items-center justify-center gap-1.5 rounded-md border border-gray-300 bg-white px-3 text-xs font-medium text-gray-700 disabled:opacity-50"
                            >
                              キューを破棄
                            </button>
                          </>
                        )}
                        {continuousLearn.phase === "completed" && (
                          <button
                            type="button"
                            onClick={discardContinuousLearn}
                            disabled={busyAction !== null}
                            className="inline-flex h-9 items-center justify-center gap-1.5 rounded-md border border-gray-300 bg-white px-3 text-xs font-medium text-gray-700 disabled:opacity-50"
                          >
                            <RotateCcw size={14} /> 新しいキューを作る
                          </button>
                        )}
                      </div>
                    </div>
                  )}

                  {learnNotice && (
                    <p className="mt-3 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700" aria-live="polite">
                      {learnNotice}
                    </p>
                  )}
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

                  {!hasRoleAssignments && roleControls.length > 0 && (
                    <div className="mt-5 rounded-lg border border-dashed border-blue-200 bg-blue-50 p-6 text-sm text-blue-800">
                      デバイス設定タブで、このRoleに1台以上のデバイスを割り当ててください。論理Controlの一覧は先に確認できます。
                    </div>
                  )}

                  {roleControls.length === 0 ? (
                    <div className="mt-5 rounded-lg border border-dashed border-gray-300 bg-gray-50 p-6 text-sm text-gray-500">
                      <p>このRoleには表示可能な論理Controlがありません。</p>
                      <p className="mt-1">
                        {hasRoleAssignments
                          ? "割り当て済みデバイスのControl数が0、または未対応のDevice kindです。"
                          : "Role定義に論理Controlが定義されていません。"}
                      </p>
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
                        const draft = bindingDrafts[control.logicalControlId];
                        const draftDeviceId = roleAssignments.some(
                          (assignment) => assignment.deviceId === draft?.deviceId,
                        )
                          ? draft.deviceId
                          : roleAssignments[0]?.deviceId ?? "";
                        const draftDevice = devices.find((device) => device.deviceId === draftDeviceId) ?? null;
                        const draftPhysicalControls = getPhysicalControlCatalog(
                          draftDevice?.deviceKindId ?? null,
                          draftDevice?.controls ?? null,
                        );
                        const draftPhysicalControlId = draftPhysicalControls.some(
                          (physical) => String(physical.physicalControlId) === draft?.physicalControlId,
                        )
                          ? draft.physicalControlId
                          : draftPhysicalControls[0]
                            ? String(draftPhysicalControls[0].physicalControlId)
                            : "";
                        const learningThisControl =
                          learnSession.active &&
                          learnSession.roleId === selectedRoleId &&
                          learnSession.logicalControlId === control.logicalControlId;
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
                            <div className="mt-3 grid gap-2 rounded-md border border-gray-200 bg-white p-3 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto_auto]">
                              <label className="space-y-1 text-xs text-gray-600">
                                <span>Device</span>
                                <select
                                  value={draftDeviceId}
                                  onChange={(event) => {
                                    const nextDeviceId = event.target.value;
                                    const nextDevice = devices.find((device) => device.deviceId === nextDeviceId);
                                    const firstPhysical = getPhysicalControlCatalog(
                                      nextDevice?.deviceKindId ?? null,
                                      nextDevice?.controls ?? null,
                                    )[0];
                                    setBindingDrafts((current) => ({
                                      ...current,
                                      [control.logicalControlId]: {
                                        deviceId: nextDeviceId,
                                        physicalControlId: firstPhysical ? String(firstPhysical.physicalControlId) : "",
                                      },
                                    }));
                                  }}
                                  disabled={!hasRoleAssignments || busyAction !== null}
                                  className="w-full rounded-md border border-gray-300 bg-white px-2 py-2 text-xs"
                                >
                                  {!hasRoleAssignments && (
                                    <option value="">デバイスを割り当ててください</option>
                                  )}
                                  {roleAssignments.map((assignment) => (
                                    <option key={assignment.deviceId} value={assignment.deviceId}>
                                      {devices.find((device) => device.deviceId === assignment.deviceId)?.displayName ?? assignment.deviceId}
                                    </option>
                                  ))}
                                </select>
                              </label>
                              <label className="space-y-1 text-xs text-gray-600">
                                <span>Physical Control ID</span>
                                <select
                                  value={draftPhysicalControlId}
                                  onChange={(event) => setBindingDrafts((current) => ({
                                    ...current,
                                    [control.logicalControlId]: {
                                      deviceId: draftDeviceId,
                                      physicalControlId: event.target.value,
                                    },
                                  }))}
                                  disabled={!hasRoleAssignments || draftPhysicalControls.length === 0 || busyAction !== null}
                                  className="w-full rounded-md border border-gray-300 bg-white px-2 py-2 text-xs disabled:bg-gray-100"
                                >
                                  {draftPhysicalControls.length === 0 && (
                                    <option value="">
                                      {hasRoleAssignments ? "物理Control一覧なし" : "デバイス割当後に選択できます"}
                                    </option>
                                  )}
                                  {draftPhysicalControls.map((physical) => (
                                    <option key={physical.physicalControlId} value={physical.physicalControlId}>
                                      {physical.label} · ID {physical.physicalControlId}
                                    </option>
                                  ))}
                                </select>
                              </label>
                              <button
                                type="button"
                                onClick={() => void addManualBinding(
                                  control.logicalControlId,
                                  draftDeviceId,
                                  draftPhysicalControlId,
                                )}
                                disabled={
                                  busyAction !== null ||
                                  !hasRoleAssignments ||
                                  !draftDeviceId ||
                                  !draftPhysicalControlId
                                }
                                className="inline-flex h-9 items-center justify-center gap-1.5 self-end rounded-md bg-blue-600 px-3 text-xs font-medium text-white disabled:opacity-50"
                              >
                                <Plus size={14} /> 追加
                              </button>
                              <button
                                type="button"
                                onClick={() => learningThisControl
                                  ? void handleLearnCancel()
                                  : void startSingleLearn(control.logicalControlId, draftDeviceId)}
                                disabled={
                                  busyAction !== null ||
                                  (learnSession.active && !learningThisControl) ||
                                  continuousLearn?.phase === "arming" ||
                                  continuousLearn?.phase === "canceling" ||
                                  (continuousLearn !== null &&
                                    continuousLearn.phase !== "paused" &&
                                    !learningThisControl) ||
                                  (!learningThisControl && (!hasRoleAssignments || !draftDeviceId))
                                }
                                className={`inline-flex h-9 items-center justify-center gap-1.5 self-end rounded-md px-3 text-xs font-medium disabled:opacity-50 ${
                                  learningThisControl
                                    ? "border border-red-200 bg-red-50 text-red-700"
                                    : "bg-indigo-600 text-white"
                                }`}
                              >
                                <Radio size={14} /> {learningThisControl ? "学習取消" : "学習"}
                              </button>
                            </div>
                            {hasRoleAssignments && draftPhysicalControls.length === 0 && (
                              <p className="mt-2 text-xs text-amber-700">
                                このデバイスの物理Control一覧を利用できません。学習で受信イベントを登録できます。
                              </p>
                            )}
                            {learningThisControl && (
                              <p className="mt-2 text-xs text-indigo-700">
                                {draftDeviceId ? `${draftDevice?.displayName ?? draftDeviceId} の` : "次の"}物理入力を待っています。
                              </p>
                            )}
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
                            <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-gray-200 pt-3">
                              <span className="mr-1 text-xs font-medium text-gray-500">Role操作</span>
                              {control.supportedEvents.map((supportedEvent) => {
                                const actionKey = `${control.logicalControlId}:${supportedEvent}`;
                                return (
                                  <button
                                    key={actionKey}
                                    type="button"
                                    onClick={() => void triggerRoleAction(control.logicalControlId, supportedEvent)}
                                    disabled={busyAction !== null || triggeringRoleAction !== null}
                                    className="inline-flex items-center gap-1.5 rounded-md border border-blue-200 bg-white px-2.5 py-1.5 text-xs text-blue-700 transition hover:bg-blue-50 active:scale-95 disabled:opacity-50"
                                  >
                                    <Play size={12} />
                                    {triggeringRoleAction === actionKey ? "実行中…" : eventLabels[supportedEvent]}
                                  </button>
                                );
                              })}
                              {roleActionNotice?.controlId === control.logicalControlId && (
                                <span className="text-xs text-gray-600">{roleActionNotice.message}</span>
                              )}
                            </div>
                          </div>
                        );
                      })}
                    </div>
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
