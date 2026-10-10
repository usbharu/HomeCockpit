"use client";

import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ChevronRight,
  Cable,
  Plane,
  Plus,
  Power,
  Server,
} from "lucide-react";

import type {
  DcsBiosConnectionConfig,
  DcsBiosStatus,
  McpConfig,
  McpStatus,
} from "@/lib/manager-types";

type SoftwareSettingsProps = {
  config: DcsBiosConnectionConfig;
  status: DcsBiosStatus;
  mcpConfig: McpConfig;
  mcpStatus: McpStatus;
  runtimeError: string | null;
  busyAction: string | null;
  onSave: (config: DcsBiosConnectionConfig) => Promise<void>;
  onStart: () => Promise<void>;
  onStop: () => Promise<void>;
  onSaveMcp: (config: McpConfig) => Promise<void>;
};

type SoftwareId = "dcs-bios";

type SoftwareDefinition = {
  id: SoftwareId;
  name: string;
  shortDescription: string;
};

const softwareCatalog: SoftwareDefinition[] = [
  {
    id: "dcs-bios",
    name: "DCS-BIOS",
    shortDescription: "DCS World との入出力ブリッジ",
  },
];

const metricCardClass = "rounded-lg border border-gray-200 bg-white p-5 shadow-sm";

export const SoftwareSettings = ({
  config,
  status,
  mcpConfig,
  mcpStatus,
  runtimeError,
  busyAction,
  onSave,
  onStart,
  onStop,
  onSaveMcp,
}: SoftwareSettingsProps) => {
  const [draft, setDraft] = useState(config);
  const [mcpDraft, setMcpDraft] = useState(mcpConfig);
  const [isAddMenuOpen, setIsAddMenuOpen] = useState(false);
  const [addedSoftwareIds, setAddedSoftwareIds] = useState<SoftwareId[]>(["dcs-bios"]);
  const [selectedSoftwareId, setSelectedSoftwareId] = useState<SoftwareId>("dcs-bios");

  useEffect(() => {
    setDraft(config);
  }, [config]);

  useEffect(() => {
    setMcpDraft(mcpConfig);
  }, [mcpConfig]);

  const addedSoftwares = useMemo(
    () => softwareCatalog.filter((software) => addedSoftwareIds.includes(software.id)),
    [addedSoftwareIds],
  );

  const selectedSoftware =
    softwareCatalog.find((software) => software.id === selectedSoftwareId) ?? null;

  const handleAddSoftware = (softwareId: SoftwareId) => {
    setAddedSoftwareIds((current) =>
      current.includes(softwareId) ? current : [...current, softwareId],
    );
    setSelectedSoftwareId(softwareId);
    setIsAddMenuOpen(false);
  };

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="mx-auto flex max-w-7xl flex-col gap-6">
        <div className="grid gap-6 xl:grid-cols-[340px_minmax(0,1fr)]">
          <aside className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
            <div className="flex items-start justify-between gap-4">
              <div>
                <h3 className="text-xl font-semibold text-gray-900">接続リスト</h3>
              </div>
              <div className="relative">
                <button
                  type="button"
                  onClick={() => setIsAddMenuOpen((current) => !current)}
                  className="inline-flex items-center gap-2 rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-700"
                >
                  <Plus size={16} />
                  追加
                </button>

                {isAddMenuOpen && (
                  <div className="absolute right-0 top-12 z-10 w-64 rounded-lg border border-gray-200 bg-white p-2 shadow-lg">
                    {softwareCatalog.map((software) => {
                      const isAdded = addedSoftwareIds.includes(software.id);

                      return (
                        <button
                          key={software.id}
                          type="button"
                          onClick={() => handleAddSoftware(software.id)}
                          className="flex w-full items-center justify-between rounded-md px-3 py-3 text-left transition hover:bg-gray-50"
                        >
                          <div>
                            <p className="font-medium text-gray-900">{software.name}</p>
                          </div>
                          <span className="rounded-full bg-gray-100 px-2.5 py-1 text-xs text-gray-600">
                            {isAdded ? "追加済み" : "追加"}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>

            <div className="mt-5 space-y-3">
              {addedSoftwares.map((software) => {
                const isSelected = software.id === selectedSoftwareId;

                return (
                  <button
                    key={software.id}
                    type="button"
                    onClick={() => setSelectedSoftwareId(software.id)}
                    className={`w-full rounded-lg border p-4 text-left transition ${
                      isSelected
                        ? "border-blue-200 bg-blue-50"
                        : "border-gray-200 bg-white hover:border-gray-300 hover:bg-gray-50"
                    }`}
                  >
                    <div className="flex items-start gap-3">
                      <div className="mt-0.5 flex h-11 w-11 items-center justify-center rounded-lg bg-gray-100 text-blue-600">
                        <Cable size={18} />
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center justify-between gap-3">
                          <p className="font-semibold text-gray-900">{software.name}</p>
                          <ChevronRight
                            size={18}
                            className={isSelected ? "text-blue-600" : "text-gray-400"}
                          />
                        </div>
                        <div className="mt-3">
                          <span className="rounded-full border border-gray-200 bg-gray-50 px-2.5 py-1 text-xs text-gray-600">
                            {status.connectionState}
                          </span>
                        </div>
                      </div>
                    </div>
                  </button>
                );
              })}
            </div>
          </aside>

          <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
            {!selectedSoftware ? (
              <div className="flex h-full min-h-[420px] flex-col items-center justify-center rounded-lg border border-dashed border-gray-300 bg-gray-50 px-8 text-center">
                <Server className="text-gray-400" size={28} />
                <h3 className="mt-4 text-xl font-semibold text-gray-900">接続対象なし</h3>
              </div>
            ) : (
              <div className="space-y-6">
                <div className="flex flex-col gap-4 rounded-lg border border-gray-200 bg-gray-50 p-6 lg:flex-row lg:items-start lg:justify-between">
                  <div className="flex items-start gap-4">
                    <div className="flex h-14 w-14 items-center justify-center rounded-lg bg-gray-100 text-blue-600">
                      <Cable size={24} />
                    </div>
                    <div>
                      <h3 className="text-3xl font-semibold tracking-tight text-gray-900">
                        {selectedSoftware.name}
                      </h3>
                    </div>
                  </div>

                  <div className="flex flex-wrap gap-3">
                    <button
                      type="button"
                      onClick={() => onSave(draft)}
                      disabled={busyAction !== null}
                      className="rounded-md border border-gray-300 bg-white px-5 py-2.5 text-sm font-medium text-gray-700 transition hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      保存
                    </button>
                    <button
                      type="button"
                      onClick={async () => {
                        await onSave(draft);
                        await onStart();
                      }}
                      disabled={busyAction !== null}
                      className="inline-flex items-center gap-2 rounded-md bg-blue-600 px-5 py-2.5 text-sm font-medium text-white transition hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      <Power size={16} />
                      保存して開始
                    </button>
                    <button
                      type="button"
                      onClick={onStop}
                      disabled={busyAction !== null}
                      className="rounded-md border border-red-200 bg-red-50 px-5 py-2.5 text-sm font-medium text-red-700 transition hover:bg-red-100 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                      停止
                    </button>
                  </div>
                </div>

                {(runtimeError || status.error) && (
                  <section className="rounded-lg border border-amber-200 bg-amber-50 p-4 text-amber-900">
                    <div className="flex items-start gap-3">
                      <AlertTriangle className="mt-0.5 text-amber-600" size={18} />
                      <div>
                        <p className="font-medium">実行時エラー</p>
                        <p className="text-sm">{runtimeError ?? status.error}</p>
                      </div>
                    </div>
                  </section>
                )}

                <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <div>
                    <h4 className="text-xl font-semibold text-gray-900">接続設定</h4>
                  </div>

                  <div className="mt-5 space-y-5">
                    <div>
                      <p className="text-sm font-medium text-gray-700">受信設定</p>
                      <div className="mt-3 grid gap-4 md:grid-cols-2">
                        <label className="space-y-2 text-sm text-gray-600">
                          <span>受信ホスト</span>
                          <input
                            value={draft.exportHost}
                            onChange={(event) =>
                              setDraft({ ...draft, exportHost: event.target.value })
                            }
                            className="w-full rounded-md border border-gray-300 bg-white px-4 py-3 text-gray-900 outline-none focus:border-blue-500"
                          />
                        </label>
                        <label className="space-y-2 text-sm text-gray-600">
                          <span>受信ポート</span>
                          <input
                            type="number"
                            value={draft.exportPort}
                            onChange={(event) =>
                              setDraft({
                                ...draft,
                                exportPort: Number(event.target.value),
                              })
                            }
                            className="w-full rounded-md border border-gray-300 bg-white px-4 py-3 text-gray-900 outline-none focus:border-blue-500"
                          />
                        </label>
                      </div>
                    </div>

                    <div>
                      <p className="text-sm font-medium text-gray-700">送信設定</p>
                      <div className="mt-3 grid gap-4 md:grid-cols-3">
                        <label className="space-y-2 text-sm text-gray-600">
                          <span>送信ホスト</span>
                          <input
                            value={draft.commandHost}
                            onChange={(event) =>
                              setDraft({ ...draft, commandHost: event.target.value })
                            }
                            className="w-full rounded-md border border-gray-300 bg-white px-4 py-3 text-gray-900 outline-none focus:border-blue-500"
                          />
                        </label>
                        <label className="space-y-2 text-sm text-gray-600">
                          <span>送信ポート</span>
                          <input
                            type="number"
                            value={draft.commandPort}
                            onChange={(event) =>
                              setDraft({
                                ...draft,
                                commandPort: Number(event.target.value),
                              })
                            }
                            className="w-full rounded-md border border-gray-300 bg-white px-4 py-3 text-gray-900 outline-none focus:border-blue-500"
                          />
                        </label>
                        <label className="space-y-2 text-sm text-gray-600">
                          <span>プロトコル</span>
                          <select
                            value={draft.commandTransport}
                            onChange={(event) =>
                              setDraft({
                                ...draft,
                                commandTransport:
                                  event.target.value as DcsBiosConnectionConfig["commandTransport"],
                              })
                            }
                            className="w-full rounded-md border border-gray-300 bg-white px-4 py-3 text-gray-900 outline-none focus:border-blue-500"
                          >
                            <option value="udp">UDP</option>
                            <option value="tcp">TCP</option>
                          </select>
                        </label>
                      </div>
                    </div>
                  </div>
                </section>

                <div className="grid gap-4 lg:grid-cols-4">
                  <section className={metricCardClass}>
                    <p className="text-sm text-gray-500">接続状態</p>
                    <p className="mt-3 text-3xl font-semibold capitalize text-gray-900">
                      {status.connectionState}
                    </p>
                    <p className="mt-2 text-sm text-gray-500">
                      Export {config.exportHost}:{config.exportPort}
                    </p>
                  </section>
                  <section className={metricCardClass}>
                    <p className="text-sm text-gray-500">受信レート</p>
                    <p className="mt-3 text-3xl font-semibold text-gray-900">
                      {status.packetsPerSecond}/s
                    </p>
                    <p className="mt-2 text-sm text-gray-500">
                      累計 {status.totalPackets} パケット
                    </p>
                  </section>
                  <section className={metricCardClass}>
                    <p className="text-sm text-gray-500">最終受信</p>
                    <p className="mt-3 text-xl font-semibold text-gray-900">
                      {status.lastSeenAt
                        ? new Date(status.lastSeenAt).toLocaleTimeString()
                        : "未受信"}
                    </p>
                    <p className="mt-2 text-sm text-gray-500">
                      {config.commandTransport.toUpperCase()} {config.commandHost}:
                      {config.commandPort}
                    </p>
                  </section>
                  <section className={metricCardClass}>
                    <p className="text-sm text-gray-500">状態情報</p>
                    <div className="mt-3 flex items-center gap-3 text-gray-900">
                      <Plane className="text-sky-600" size={24} />
                      <p className="text-xl font-semibold">
                        {status.aircraftName ?? "未取得"}
                      </p>
                    </div>
                  </section>
                </div>

                <details className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                  <summary className="cursor-pointer list-none [&::-webkit-details-marker]:hidden">
                    <h4 className="text-xl font-semibold text-gray-900">接続診断</h4>
                  </summary>

                  <div className="mt-5 grid gap-3">
                    {status.diagnostics.length === 0 ? (
                      <p className="text-sm text-gray-500">診断メッセージはありません。</p>
                    ) : (
                      status.diagnostics.map((diagnostic) => (
                        <div key={diagnostic} className="rounded-lg border border-gray-200 bg-gray-50 px-4 py-3 text-sm text-gray-700">
                          {diagnostic}
                        </div>
                      ))
                    )}
                  </div>
                </details>
              </div>
            )}
          </section>
        </div>
        <section className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
          <h3 className="text-lg font-semibold text-gray-900">外部連携（MCP サーバー）</h3>
          <div className="mt-4 flex flex-wrap items-end gap-4">
            <label className="flex items-center gap-2 text-sm text-gray-700">
              <input
                type="checkbox"
                checked={mcpDraft.enabled}
                onChange={(event) => setMcpDraft({ ...mcpDraft, enabled: event.target.checked })}
              />
              有効にする
            </label>
            <label className="space-y-1 text-sm text-gray-700">
              <span className="block">ポート</span>
              <input
                type="number"
                min={1}
                max={65535}
                value={mcpDraft.port}
                onChange={(event) => setMcpDraft({ ...mcpDraft, port: Number(event.target.value) })}
                className="w-32 rounded-md border border-gray-300 px-3 py-2"
              />
            </label>
            <button
              type="button"
              onClick={() => void onSaveMcp(mcpDraft)}
              disabled={mcpDraft.port < 1 || mcpDraft.port > 65535 || busyAction === "save-mcp-config"}
              className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
            >
              保存
            </button>
          </div>
          <p className="mt-3 text-sm text-gray-600">状態: {mcpStatus.state}</p>
          {mcpStatus.url && <p className="mt-1 break-all text-sm text-gray-700">接続先: {mcpStatus.url}</p>}
          {mcpStatus.error && (
            <p role="alert" className="mt-2 text-sm text-red-700">{mcpStatus.error}</p>
          )}
        </section>
      </div>
    </div>
  );
};
