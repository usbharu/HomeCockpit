export const DeviceDetail = ({ device }) => {
    if (!device) {
        return <div className="p-8 text-center text-gray-500">デバイスを選択してください。</div>;
    }

    return (
        <div className="p-8 space-y-6">
            <div className="flex items-start justify-between">
                <div>
                    {device.editing ? (
                        <input
                            type="text"
                            value={device.name}
                            onChange={(e) => handleNameChange(device.id, e.target.value)}
                            onBlur={() => toggleEditing(device.id)}
                            className="text-2xl font-semibold text-gray-800 bg-transparent border-b-2 border-blue-500 focus:outline-none"
                            autoFocus
                        />
                    ) : (
                        <h2 className="text-2xl font-semibold text-gray-800" onClick={() => toggleEditing(device.id)}>{device.name}</h2>
                    )}
                    <p className="text-sm text-gray-500">デバイスタイプ: {device.type}</p>
                </div>
                <div className="flex items-center space-x-4 text-sm">
                    {device.battery !== null && <span className="text-gray-600">バッテリー: {device.battery}%</span>}
                    <span className="bg-gray-200 text-gray-700 px-2 py-1 rounded">FW: {device.firmware}</span>
                </div>
            </div>

            <div className="bg-white p-6 rounded-lg shadow-sm border border-gray-200 space-y-4">
                <h3 className="font-semibold text-gray-700">基本操作</h3>
                <div className="flex space-x-4">
                    <button className="bg-gray-100 text-gray-800 px-4 py-2 rounded-md hover:bg-gray-200 transition-colors font-medium text-sm">
                        キャリブレーション
                    </button>
                    <button className="bg-gray-100 text-gray-800 px-4 py-2 rounded-md hover:bg-gray-200 transition-colors font-medium text-sm">
                        設定バックアップ
                    </button>
                    <button className="bg-gray-100 text-gray-800 px-4 py-2 rounded-md hover:bg-gray-200 transition-colors font-medium text-sm">
                        設定を復元
                    </button>
                </div>
            </div>
            <div className="bg-white p-6 rounded-lg shadow-sm border border-gray-200 space-y-4">
                <h3 className="font-semibold text-gray-700">危険な操作</h3>
                <div className="flex items-center justify-between">
                    <p className="text-sm text-gray-600">このデバイスの登録を解除します。再接続には初期設定が必要です。</p>
                    <button className="bg-red-100 text-red-700 px-4 py-2 rounded-md hover:bg-red-200 transition-colors font-medium text-sm">
                        デバイス登録解除
                    </button>
                </div>
            </div>
        </div>
    );
}