"use client"

import {useState} from "react";
import { SoftwareConnectItem } from "../software-connect/software-connect-item";

export const SoftwareSettings = () => {
    const [softwareList, setSoftwareList] = useState([
        { id: 1, name: 'OBS Studio', connected: true, ip: '127.0.0.1', port: '4455' },
        { id: 2, name: 'Streamlabs Desktop', connected: false, ip: '192.168.1.10', port: '8080' },
        { id: 3, name: 'VMagicMirror', connected: true, ip: '127.0.0.1', port: '54321' },
    ]);

    const toggleConnection = (id) => {
        setSoftwareList(softwareList.map(sw =>
            sw.id === id ? { ...sw, connected: !sw.connected } : sw
        ));
    };

    return (
        <div className="p-8 space-y-6">
            <h2 className="text-2xl font-semibold text-gray-800">ソフトウェア接続設定</h2>
            <div className="bg-white p-6 rounded-lg shadow-sm border border-gray-200 divide-y divide-gray-200">
                {softwareList.map(sw => (
                   <SoftwareConnectItem key={sw.id} sw={sw} />
                ))}
            </div>
            <div className="pt-6">
                <h3 className="text-lg font-semibold text-gray-700 mb-4">新規接続の追加</h3>
                <div className="bg-white p-6 rounded-lg shadow-sm border border-gray-200 space-y-4">
                    <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                        <div>
                            <label className="text-sm font-medium text-gray-600 block mb-1">ソフトウェア名</label>
                            <input type="text" placeholder="例: OBS Studio" className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500" />
                        </div>
                        <div>
                            <label className="text-sm font-medium text-gray-600 block mb-1">IPアドレス</label>
                            <input type="text" placeholder="127.0.0.1" className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500" />
                        </div>
                        <div>
                            <label className="text-sm font-medium text-gray-600 block mb-1">ポート</label>
                            <input type="text" placeholder="4455" className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500" />
                        </div>
                    </div>
                    <div className="flex justify-end">
                        <button className="bg-blue-600 text-white px-5 py-2 rounded-md hover:bg-blue-700 transition-colors font-medium">
                            テスト接続
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
};
