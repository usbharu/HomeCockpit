"use client";

import React from 'react';

export const StatusPage = () => {
    const logs = [
        { time: '14:32:10', level: 'INFO', message: 'アプリを起動しました。バージョン 1.0.0' },
        { time: '14:32:12', level: 'SUCCESS', message: 'デバイス "My Custom Controller" が接続されました。' },
        { time: '14:32:15', level: 'SUCCESS', message: 'ソフトウェア "OBS Studio" に接続しました。' },
        { time: '14:33:01', level: 'WARN', message: 'デバイス "Foot Pedal" のバッテリー残量が低下しています (20%)。' },
        { time: '14:33:20', level: 'INFO', message: '入力: Button A - Press' },
        { time: '14:33:25', level: 'ERROR', message: 'ソフトウェア "Streamlabs Desktop" への接続に失敗しました。' },
    ];

    const getLevelColor = (level) => {
        switch(level) {
            case 'SUCCESS': return 'text-green-600';
            case 'WARN': return 'text-yellow-600';
            case 'ERROR': return 'text-red-600';
            default: return 'text-gray-600';
        }
    };

    return (
        <div className="p-8 space-y-6">
            <h2 className="text-2xl font-semibold text-gray-800">ステータス</h2>
            <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
                <div className="bg-white p-4 rounded-lg shadow-sm border"><p className="text-sm text-gray-500">CPU使用率</p><p className="text-2xl font-bold">12%</p></div>
                <div className="bg-white p-4 rounded-lg shadow-sm border"><p className="text-sm text-gray-500">メモリ使用量</p><p className="text-2xl font-bold">128 MB</p></div>
                <div className="bg-white p-4 rounded-lg shadow-sm border"><p className="text-sm text-gray-500">データ送信レート</p><p className="text-2xl font-bold">256 Kbps</p></div>
            </div>
            <div>
                <h3 className="text-lg font-semibold text-gray-700 mb-2">リアルタイムログ</h3>
                <div className="bg-gray-900 text-white font-mono text-sm p-4 rounded-lg h-80 overflow-y-auto">
                    {logs.slice().reverse().map((log, index) => (
                        <p key={index}>
                            <span className="text-gray-500">{log.time}</span>
                            <span className={`font-bold ml-2 ${getLevelColor(log.level)}`}> [{log.level}] </span>
                            <span>{log.message}</span>
                        </p>
                    ))}
                </div>
            </div>
        </div>
    );
};

export default StatusPage;

