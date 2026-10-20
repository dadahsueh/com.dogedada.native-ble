using System;
using System.Collections.Generic;
using System.Text;
using UnityEngine;

namespace UnityNativeBLE
{
    [Serializable]
    public class BLEDevice
    {
        public string id;
        public string name;
        public string address;
        public int rssi;
        public List<string> services = new List<string>();
    }

    [Serializable]
    public class GattCharacteristicInfo
    {
        public string uuid;
        public bool can_read;
        public bool can_write;
        public bool can_write_without_response;
        public bool can_notify;
        public bool can_indicate;
        public List<string> descriptors = new List<string>();
    }

    [Serializable]
    public class GattServiceInfo
    {
        public string uuid;
        public bool primary;
        public List<GattCharacteristicInfo> characteristics = new List<GattCharacteristicInfo>();
    }

    [Serializable]
    internal class RawBleEvent
    {
        public string type;
        public string id;
        public string name;
        public string address;
        public int rssi;
        public List<string> services;
        public string characteristic;
        public int[] data;
        public string message;
        public string client;
        public bool subscribed;
    }

    [Serializable]
    internal class DeviceListWrapper
    {
        public List<BLEDevice> devices;
    }

    [Serializable]
    internal class ServiceListWrapper
    {
        public List<GattServiceInfo> services;
    }

    /// <summary>
    /// High-level Unity BLE Manager.
    /// Provides event-driven, thread-safe Bluetooth LE operations for Unity.
    /// </summary>
    public class BluetoothManager : MonoBehaviour
    {
        private static BluetoothManager _instance;
        public static BluetoothManager Instance
        {
            get
            {
                if (_instance == null)
                {
                    var go = new GameObject("BluetoothManager");
                    _instance = go.AddComponent<BluetoothManager>();
                    DontDestroyOnLoad(go);
                }
                return _instance;
            }
        }

        public event Action<BLEDevice> OnDeviceDiscovered;
        public event Action<string> OnConnected;
        public event Action<string> OnDisconnected;
        public event Action<string, string, byte[]> OnNotificationReceived;
        public event Action<string> OnError;

        // Peripheral Events
        public event Action<string, string, byte[]> OnPeripheralWriteReceived;
        public event Action<string, string, bool> OnPeripheralSubscriptionChanged;

        private readonly byte[] _pollBuffer = new byte[8192];
        private bool _isInitialized = false;

        private void Awake()
        {
            if (_instance == null)
            {
                _instance = this;
                DontDestroyOnLoad(gameObject);
            }
            else if (_instance != this)
            {
                Destroy(gameObject);
            }
        }

        private void OnDestroy()
        {
            if (_isInitialized)
            {
                Cleanup();
            }
        }

        /// <summary>
        /// Initialize the BLE native subsystem.
        /// On Android, automatically bridges Unity's JVM and Activity via JNI.
        /// </summary>
        public bool Initialize()
        {
            if (_isInitialized) return true;

#if UNITY_ANDROID && !UNITY_EDITOR
            try
            {
                IntPtr jvm = AndroidJNI.GetJavaVM();
                using (var unityPlayer = new AndroidJavaClass("com.unity3d.player.UnityPlayer"))
                using (var activity = unityPlayer.GetStatic<AndroidJavaObject>("currentActivity"))
                {
                    IntPtr activityPtr = activity.GetRawObject();
                    NativeBLE.BLEInitAndroid(jvm, activityPtr);
                }
            }
            catch (Exception ex)
            {
                Debug.LogWarning($"[Native BLE] Android JNI initialization attempt: {ex.Message}");
            }
#elif (UNITY_OPENHARMONY || UNITY_OHOS) && !UNITY_EDITOR
            try
            {
                NativeBLE.BLEInitOHOS(IntPtr.Zero, IntPtr.Zero);
            }
            catch (Exception ex)
            {
                Debug.LogWarning($"[Native BLE] OpenHarmony initialization attempt: {ex.Message}");
            }
#endif

            int res = NativeBLE.BLEInit();
            _isInitialized = (res == 0);
            return _isInitialized;
        }

        /// <summary>
        /// Initialize the BLE native subsystem for OpenHarmony with explicit N-API env and context pointers.
        /// </summary>
        public bool InitializeOHOS(IntPtr env, IntPtr abilityContext)
        {
            if (_isInitialized) return true;
            try
            {
                NativeBLE.BLEInitOHOS(env, abilityContext);
            }
            catch (Exception ex)
            {
                Debug.LogWarning($"[Native BLE] OpenHarmony explicit initialization attempt: {ex.Message}");
            }
            int res = NativeBLE.BLEInit();
            _isInitialized = (res == 0);
            return _isInitialized;
        }

        /// <summary>
        /// Cleanup all BLE connections and tasks.
        /// </summary>
        public void Cleanup()
        {
            if (!_isInitialized) return;
            NativeBLE.BLECleanup();
            _isInitialized = false;
        }

        /// <summary>
        /// Get number of available BLE adapters.
        /// </summary>
        public int GetAdapterCount()
        {
            return NativeBLE.BLEGetAdapterCount();
        }

        /// <summary>
        /// Start scanning for peripherals. Discovered devices are delivered via OnDeviceDiscovered.
        /// </summary>
        public bool StartScan()
        {
            if (!_isInitialized && !Initialize()) return false;
            return NativeBLE.BLEStartScan() == 0;
        }

        /// <summary>
        /// Stop active scanning.
        /// </summary>
        public bool StopScan()
        {
            return NativeBLE.BLEStopScan() == 0;
        }

        /// <summary>
        /// Get the list of all currently discovered devices.
        /// </summary>
        public List<BLEDevice> GetDiscoveredDevices()
        {
            byte[] buf = new byte[16384];
            int len = NativeBLE.BLEGetDiscoveredDevicesJson(buf, (uint)buf.Length);
            if (len <= 0) return new List<BLEDevice>();

            string json = Encoding.UTF8.GetString(buf, 0, len);
            // JsonUtility cannot parse top-level JSON arrays directly, so wrap it
            string wrapped = "{\"devices\":" + json + "}";
            try
            {
                var wrapper = JsonUtility.FromJson<DeviceListWrapper>(wrapped);
                return wrapper?.devices ?? new List<BLEDevice>();
            }
            catch (Exception ex)
            {
                Debug.LogError($"[BluetoothManager] Failed to parse discovered devices: {ex.Message}");
                return new List<BLEDevice>();
            }
        }

        /// <summary>
        /// Connect to a device by ID.
        /// </summary>
        public bool Connect(string deviceId)
        {
            if (string.IsNullOrEmpty(deviceId)) return false;
            return NativeBLE.BLEConnect(deviceId) == 0;
        }

        /// <summary>
        /// Disconnect from a device by ID.
        /// </summary>
        public bool Disconnect(string deviceId)
        {
            if (string.IsNullOrEmpty(deviceId)) return false;
            return NativeBLE.BLEDisconnect(deviceId) == 0;
        }

        /// <summary>
        /// Check if a device is connected.
        /// </summary>
        public bool IsConnected(string deviceId)
        {
            if (string.IsNullOrEmpty(deviceId)) return false;
            return NativeBLE.BLEIsConnected(deviceId) == 1;
        }

        /// <summary>
        /// Discover GATT services for a connected device.
        /// </summary>
        public bool DiscoverServices(string deviceId)
        {
            if (string.IsNullOrEmpty(deviceId)) return false;
            return NativeBLE.BLEDiscoverServices(deviceId) == 0;
        }

        /// <summary>
        /// Retrieve the GATT profile (services, characteristics, descriptors) for a device.
        /// </summary>
        public List<GattServiceInfo> GetGattProfile(string deviceId)
        {
            if (string.IsNullOrEmpty(deviceId)) return new List<GattServiceInfo>();
            byte[] buf = new byte[32768];
            int len = NativeBLE.BLEGetGattProfileJson(deviceId, buf, (uint)buf.Length);
            if (len <= 0) return new List<GattServiceInfo>();

            string json = Encoding.UTF8.GetString(buf, 0, len);
            string wrapped = "{\"services\":" + json + "}";
            try
            {
                var wrapper = JsonUtility.FromJson<ServiceListWrapper>(wrapped);
                return wrapper?.services ?? new List<GattServiceInfo>();
            }
            catch (Exception ex)
            {
                Debug.LogError($"[BluetoothManager] Failed to parse GATT profile: {ex.Message}");
                return new List<GattServiceInfo>();
            }
        }

        /// <summary>
        /// Read characteristic data from a peripheral.
        /// </summary>
        public byte[] ReadCharacteristic(string deviceId, string characteristicUuid, int maxLen = 512)
        {
            if (string.IsNullOrEmpty(deviceId) || string.IsNullOrEmpty(characteristicUuid)) return null;
            byte[] buf = new byte[maxLen];
            BLEUuid uuid = BLEUuid.FromString(characteristicUuid);
            int len = NativeBLE.BLEReadCharacteristic(deviceId, uuid, buf, (uint)maxLen);
            if (len < 0) return null;

            byte[] result = new byte[len];
            Buffer.BlockCopy(buf, 0, result, 0, len);
            return result;
        }

        /// <summary>
        /// Write data to a characteristic.
        /// </summary>
        public bool WriteCharacteristic(string deviceId, string characteristicUuid, byte[] data, bool withoutResponse = false)
        {
            if (string.IsNullOrEmpty(deviceId) || string.IsNullOrEmpty(characteristicUuid)) return false;
            byte[] payload = data ?? new byte[0];
            BLEUuid uuid = BLEUuid.FromString(characteristicUuid);
            return NativeBLE.BLEWriteCharacteristic(deviceId, uuid, payload, (uint)payload.Length, withoutResponse ? 1 : 0) == 0;
        }

        /// <summary>
        /// Subscribe to characteristic notifications/indications.
        /// </summary>
        public bool SubscribeCharacteristic(string deviceId, string characteristicUuid)
        {
            if (string.IsNullOrEmpty(deviceId) || string.IsNullOrEmpty(characteristicUuid)) return false;
            BLEUuid uuid = BLEUuid.FromString(characteristicUuid);
            return NativeBLE.BLESubscribeCharacteristic(deviceId, uuid) == 0;
        }

        /// <summary>
        /// Unsubscribe from characteristic notifications.
        /// </summary>
        public bool UnsubscribeCharacteristic(string deviceId, string characteristicUuid)
        {
            if (string.IsNullOrEmpty(deviceId) || string.IsNullOrEmpty(characteristicUuid)) return false;
            BLEUuid uuid = BLEUuid.FromString(characteristicUuid);
            return NativeBLE.BLEUnsubscribeCharacteristic(deviceId, uuid) == 0;
        }

        /// <summary>
        /// Read descriptor value.
        /// </summary>
        public byte[] ReadDescriptor(string deviceId, string characteristicUuid, string descriptorUuid, int maxLen = 512)
        {
            if (string.IsNullOrEmpty(deviceId) || string.IsNullOrEmpty(descriptorUuid)) return null;
            byte[] buf = new byte[maxLen];
            BLEUuid cUuid = string.IsNullOrEmpty(characteristicUuid) ? default : BLEUuid.FromString(characteristicUuid);
            BLEUuid dUuid = BLEUuid.FromString(descriptorUuid);
            int len = NativeBLE.BLEReadDescriptor(deviceId, cUuid, dUuid, buf, (uint)maxLen);
            if (len < 0) return null;

            byte[] result = new byte[len];
            Buffer.BlockCopy(buf, 0, result, 0, len);
            return result;
        }

        /// <summary>
        /// Write descriptor value.
        /// </summary>
        public bool WriteDescriptor(string deviceId, string characteristicUuid, string descriptorUuid, byte[] data)
        {
            if (string.IsNullOrEmpty(deviceId) || string.IsNullOrEmpty(descriptorUuid)) return false;
            byte[] payload = data ?? new byte[0];
            BLEUuid cUuid = string.IsNullOrEmpty(characteristicUuid) ? default : BLEUuid.FromString(characteristicUuid);
            BLEUuid dUuid = BLEUuid.FromString(descriptorUuid);
            return NativeBLE.BLEWriteDescriptor(deviceId, cUuid, dUuid, payload, (uint)payload.Length) == 0;
        }

        // ===== Peripheral Mode (GATT Server & Advertising) =====

        /// <summary>
        /// Whether the local peripheral is actively advertising.
        /// </summary>
        public bool IsAdvertising => NativeBLE.BLEPeripheralIsAdvertising() != 0;

        /// <summary>
        /// Define a GATT Service on the local peripheral.
        /// Must be called before StartAdvertising.
        /// </summary>
        public bool AddPeripheralService(string serviceUuid, bool primary = true)
        {
            var uuid = BLEUuid.FromString(serviceUuid);
            return NativeBLE.BLEPeripheralAddService(uuid, primary ? 1 : 0) == 0;
        }

        /// <summary>
        /// Define a GATT Characteristic on a local peripheral service.
        /// </summary>
        public bool AddPeripheralCharacteristic(string serviceUuid, string charUuid, bool canRead = true, bool canWrite = true, bool canNotify = true)
        {
            var sUuid = BLEUuid.FromString(serviceUuid);
            var cUuid = BLEUuid.FromString(charUuid);
            return NativeBLE.BLEPeripheralAddCharacteristic(sUuid, cUuid, canRead ? 1 : 0, canWrite ? 1 : 0, canNotify ? 1 : 0) == 0;
        }

        /// <summary>
        /// Start advertising as a BLE peripheral.
        /// </summary>
        /// <param name="localName">Advertised local device name.</param>
        /// <param name="serviceUuids">Optional list of service UUIDs to include in advertising packets.</param>
        public bool StartAdvertising(string localName = "UnityBLE", string[] serviceUuids = null)
        {
            if (!_isInitialized && !Initialize()) return false;
            string csv = serviceUuids != null ? string.Join(",", serviceUuids) : "";
            return NativeBLE.BLEPeripheralStartAdvertising(localName, csv) == 0;
        }

        /// <summary>
        /// Stop advertising as a BLE peripheral.
        /// </summary>
        public bool StopAdvertising()
        {
            return NativeBLE.BLEPeripheralStopAdvertising() == 0;
        }

        /// <summary>
        /// Update a characteristic value and push notifications/indications to connected central devices.
        /// </summary>
        public bool UpdateCharacteristic(string charUuid, byte[] data)
        {
            var uuid = BLEUuid.FromString(charUuid);
            return NativeBLE.BLEPeripheralUpdateCharacteristic(uuid, data, (uint)(data?.Length ?? 0)) == 0;
        }

        private void Update()
        {
            if (!_isInitialized) return;

            // Poll all pending native events this frame
            while (true)
            {
                int len = NativeBLE.BLEPollEventJson(_pollBuffer, (uint)_pollBuffer.Length);
                if (len <= 0) break;

                string json = Encoding.UTF8.GetString(_pollBuffer, 0, len);
                DispatchEvent(json);
            }
        }

        private void DispatchEvent(string json)
        {
            try
            {
                var ev = JsonUtility.FromJson<RawBleEvent>(json);
                if (ev == null) return;

                switch (ev.type)
                {
                    case "DeviceDiscovered":
                        OnDeviceDiscovered?.Invoke(new BLEDevice
                        {
                            id = ev.id,
                            name = ev.name,
                            address = ev.address,
                            rssi = ev.rssi,
                            services = ev.services ?? new List<string>()
                        });
                        break;
                    case "Connected":
                        OnConnected?.Invoke(ev.id);
                        break;
                    case "Disconnected":
                        OnDisconnected?.Invoke(ev.id);
                        break;
                    case "Notification":
                        byte[] bytes = null;
                        if (ev.data != null)
                        {
                            bytes = new byte[ev.data.Length];
                            for (int i = 0; i < ev.data.Length; i++) bytes[i] = (byte)ev.data[i];
                        }
                        OnNotificationReceived?.Invoke(ev.id, ev.characteristic, bytes ?? new byte[0]);
                        break;
                    case "PeripheralWrite":
                        byte[] pBytes = null;
                        if (ev.data != null)
                        {
                            pBytes = new byte[ev.data.Length];
                            for (int i = 0; i < ev.data.Length; i++) pBytes[i] = (byte)ev.data[i];
                        }
                        OnPeripheralWriteReceived?.Invoke(ev.client, ev.characteristic, pBytes ?? new byte[0]);
                        break;
                    case "PeripheralSubscription":
                        OnPeripheralSubscriptionChanged?.Invoke(ev.client, ev.characteristic, ev.subscribed);
                        break;
                    case "Error":
                        OnError?.Invoke(ev.message);
                        break;
                }
            }
            catch (Exception ex)
            {
                Debug.LogError($"[BluetoothManager] Failed to dispatch BLE event: {ex.Message}\nRaw JSON: {json}");
            }
        }
    }
}
