"use client";

import React from 'react';

import type { DcsBiosStatus, ManagerLogEntry } from "@/lib/manager-types";

type StatusPageProps = {
    logs: ManagerLogEntry[];
    status: DcsBiosStatus;
};

export const StatusPage = ({ logs, status }: StatusPageProps) => {
    const getLevelColor = (level: string) => {
        switch(level) {
            case 'SUCCESS': return 'text-emerald-300';
            case 'WARN': return 'text-amber-300';
            case 'ERROR': return 'text-rose-300';
            default: return 'text-slate-300';
        }
    };

    return (
        <div className="h-full overflow-y-auto p-8">
            <div className="mx-auto flex max-w-7xl flex-col gap-6">
                <div className="grid gap-4 lg:grid-cols-3">
                    <div className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
                        <p className="text-sm text-gray-500">状態</p>
                        <p className="mt-3 text-3xl font-semibold capitalize text-gray-900">{status.connectionState}</p>
                    </div>
                    <div className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
                        <p className="text-sm text-gray-500">受信パケット</p>
                        <p className="mt-3 text-3xl font-semibold text-gray-900">{status.totalPackets}</p>
                    </div>
                    <div className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
                        <p className="text-sm text-gray-500">最終更新</p>
                        <p className="mt-3 text-3xl font-semibold text-gray-900">
                            {status.lastPacketAt ? new Date(status.lastPacketAt).toLocaleTimeString() : "なし"}
                        </p>
                    </div>
                </div>
                <div>
                    <h3 className="mb-3 text-lg font-semibold text-gray-800">リアルタイムログ</h3>
                    <div className="h-[32rem] overflow-y-auto rounded-lg border border-gray-200 bg-gray-900 p-4 font-mono text-sm text-white shadow-sm">
                        {logs.length === 0 ? (
                            <p className="text-gray-500">ログはまだありません。</p>
                        ) : (
                            logs.map((log) => (
                                <p key={log.id} className="border-b border-white/5 py-2 last:border-b-0">
                                    <span className="text-gray-500">
                                        {new Date(log.at).toLocaleTimeString()}
                                    </span>
                                    <span className={`ml-2 font-bold ${getLevelColor(log.level)}`}>[{log.level}]</span>
                                    <span className="ml-2 text-gray-400">{log.source}</span>
                                    <span className="ml-2">{log.message}</span>
                                </p>
                            ))
                        )}
                    </div>
                </div>
            </div>
        </div>
    );
};

export default StatusPage;
