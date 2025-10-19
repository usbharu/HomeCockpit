export const SoftwareConnectItem = (props:{sw:any}) => {

    const sw = props.sw;

    return (
         <div  className="py-4 flex items-center justify-between">
                        <div className="flex items-center space-x-4">
                            <div className={`w-3 h-3 rounded-full ${sw.connected ? 'bg-green-500' : 'bg-gray-400'}`}></div>
                            <div>
                                <p className="font-medium text-gray-900">{sw.name}</p>
                                <p className="text-sm text-gray-500">{sw.ip}:{sw.port}</p>
                            </div>
                        </div>
                        <button
                            onClick={() => toggleConnection(sw.id)}
                            className={`px-4 py-2 text-sm font-medium rounded-md transition-colors ${
                                sw.connected
                                    ? 'bg-red-100 text-red-700 hover:bg-red-200'
                                    : 'bg-blue-100 text-blue-700 hover:bg-blue-200'
                            }`}
                        >
                            {sw.connected ? '切断' : '接続'}
                        </button>
                    </div>
    )
}