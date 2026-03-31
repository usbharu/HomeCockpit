"use client";

import React from 'react';
import { Cable, RefreshCw, Usb } from 'lucide-react';

import type { ImcpDeviceSummary } from "@/lib/manager-types";

type DeviceSettingsProps = {
    devices: ImcpDeviceSummary[];
    busyAction: string | null;
    onRefresh: () => Promise<void>;
};

const DeviceSettings = ({ devices, busyAction, onRefresh }: DeviceSettingsProps) => {
    return (
        <div className="h-full overflow-y-auto p-8">
            <div className="mx-auto flex max-w-7xl flex-col gap-6">
                <div className="flex items-center justify-between">
                    <div>
                        <h2 className="text-2xl font-semibold text-gray-800">デバイス設定</h2>
                        <p className="mt-1 text-sm text-gray-500">
                            現在接続されているデバイス候補を表示します。
                        </p>
                    </div>
                    <button
                        onClick={onRefresh}
                        disabled={busyAction !== null}
                        className="inline-flex items-center gap-2 rounded-md border border-gray-300 bg-white px-5 py-2.5 text-sm font-medium text-gray-700 transition hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                        <RefreshCw size={16} />
                        再読込
                    </button>
                </div>

                <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
                    {devices.length === 0 ? (
                        <div className="rounded-lg border border-dashed border-gray-300 bg-white p-8 text-sm text-gray-500">
                            利用可能なシリアルデバイスはありません。
                        </div>
                    ) : (
                        devices.map((device) => (
                            <section
                                key={device.id}
                                className="rounded-lg border border-gray-200 bg-white p-6 text-gray-900 shadow-sm"
                            >
                                <div className="flex items-start justify-between gap-4">
                                    <div className="flex items-center gap-3">
                                        <div className="rounded-lg bg-gray-100 p-3 text-blue-600">
                                            <Usb size={20} />
                                        </div>
                                        <div>
                                            <h3 className="font-semibold text-gray-800">{device.displayName}</h3>
                                            <p className="text-sm text-gray-500">{device.portName}</p>
                                        </div>
                                    </div>
                                    <span className="rounded-full border border-gray-200 bg-gray-100 px-3 py-1 text-xs text-gray-700">
                                        {device.state}
                                    </span>
                                </div>

                                <div className="mt-5 grid gap-3 text-sm text-gray-700">
                                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                                        <span>Transport</span>
                                        <span className="inline-flex items-center gap-2">
                                            <Cable size={14} />
                                            {device.transport}
                                        </span>
                                    </div>
                                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                                        <span>Identifier</span>
                                        <span className="truncate pl-4 text-right">{device.id}</span>
                                    </div>
                                    <div className="flex items-center justify-between rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                                        <span>Firmware</span>
                                        <span>{device.firmwareVersion ?? "Unknown"}</span>
                                    </div>
                                </div>
                            </section>
                        ))
                    )}
                </div>
            </div>
        </div>
    );
};

export default DeviceSettings;
