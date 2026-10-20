package com.dogedada.nativeble;

import android.annotation.SuppressLint;
import android.bluetooth.BluetoothAdapter;
import android.bluetooth.BluetoothDevice;
import android.bluetooth.BluetoothGatt;
import android.bluetooth.BluetoothGattCharacteristic;
import android.bluetooth.BluetoothGattDescriptor;
import android.bluetooth.BluetoothGattServer;
import android.bluetooth.BluetoothGattServerCallback;
import android.bluetooth.BluetoothGattService;
import android.bluetooth.BluetoothManager;
import android.bluetooth.BluetoothProfile;
import android.bluetooth.le.AdvertiseCallback;
import android.bluetooth.le.AdvertiseData;
import android.bluetooth.le.AdvertiseSettings;
import android.bluetooth.le.BluetoothLeAdvertiser;
import android.content.Context;
import android.os.ParcelUuid;
import android.util.Log;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;

/**
 * Clean-room Android BLE Peripheral and GATT Server Helper for Unity Native BLE.
 * Manages Advertising and GATT Server callbacks and forwards events via JNI to Rust.
 */
public class BlePeripheralHelper {
    private static final String TAG = "BlePeripheralHelper";
    private static final UUID CCCD_UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb");

    // Native JNI functions implemented in Rust
    public static native void onWriteRequest(String client, String charUuid, byte[] data);
    public static native void onSubscriptionChange(String client, String charUuid, boolean subscribed);

    private static BluetoothManager bluetoothManager;
    private static BluetoothAdapter bluetoothAdapter;
    private static BluetoothLeAdvertiser advertiser;
    private static BluetoothGattServer gattServer;
    private static boolean isAdvertising = false;

    private static final Map<UUID, BluetoothGattService> pendingServices = new HashMap<>();
    private static final Map<UUID, BluetoothGattCharacteristic> characteristicsMap = new HashMap<>();
    private static final List<BluetoothDevice> connectedDevices = new ArrayList<>();

    private static final AdvertiseCallback advertiseCallback = new AdvertiseCallback() {
        @Override
        public void onStartSuccess(AdvertiseSettings settingsInEffect) {
            super.onStartSuccess(settingsInEffect);
            isAdvertising = true;
            Log.i(TAG, "BLE Peripheral advertising started successfully.");
        }

        @Override
        public void onStartFailure(int errorCode) {
            super.onStartFailure(errorCode);
            isAdvertising = false;
            Log.e(TAG, "BLE Peripheral advertising failed with error code: " + errorCode);
        }
    };

    private static final BluetoothGattServerCallback gattServerCallback = new BluetoothGattServerCallback() {
        @Override
        public void onConnectionStateChange(BluetoothDevice device, int status, int newState) {
            super.onConnectionStateChange(device, status, newState);
            synchronized (connectedDevices) {
                if (newState == BluetoothProfile.STATE_CONNECTED) {
                    if (!connectedDevices.contains(device)) {
                        connectedDevices.add(device);
                    }
                    Log.i(TAG, "Central connected: " + device.getAddress());
                } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                    connectedDevices.remove(device);
                    Log.i(TAG, "Central disconnected: " + device.getAddress());
                }
            }
        }

        @Override
        public void onCharacteristicReadRequest(BluetoothDevice device, int requestId, int offset,
                                                BluetoothGattCharacteristic characteristic) {
            super.onCharacteristicReadRequest(device, requestId, offset, characteristic);
            byte[] value = characteristic.getValue();
            if (value == null) {
                value = new byte[0];
            }
            byte[] response = offset < value.length ? Arrays.copyOfRange(value, offset, value.length) : new byte[0];
            if (gattServer != null) {
                gattServer.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, response);
            }
        }

        @Override
        public void onCharacteristicWriteRequest(BluetoothDevice device, int requestId,
                                                 BluetoothGattCharacteristic characteristic,
                                                 boolean preparedWrite, boolean responseNeeded,
                                                 int offset, byte[] value) {
            super.onCharacteristicWriteRequest(device, requestId, characteristic, preparedWrite, responseNeeded, offset, value);
            characteristic.setValue(value);
            if (responseNeeded && gattServer != null) {
                gattServer.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value);
            }

            try {
                onWriteRequest(device.getAddress(), characteristic.getUuid().toString(), value != null ? value : new byte[0]);
            } catch (UnsatisfiedLinkError e) {
                Log.w(TAG, "Native onWriteRequest not linked: " + e.getMessage());
            }
        }

        @Override
        public void onDescriptorWriteRequest(BluetoothDevice device, int requestId,
                                             BluetoothGattDescriptor descriptor,
                                             boolean preparedWrite, boolean responseNeeded,
                                             int offset, byte[] value) {
            super.onDescriptorWriteRequest(device, requestId, descriptor, preparedWrite, responseNeeded, offset, value);
            descriptor.setValue(value);
            if (responseNeeded && gattServer != null) {
                gattServer.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value);
            }

            if (descriptor.getUuid().equals(CCCD_UUID)) {
                boolean subscribed = Arrays.equals(value, BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE) ||
                                     Arrays.equals(value, BluetoothGattDescriptor.ENABLE_INDICATION_VALUE);
                try {
                    onSubscriptionChange(device.getAddress(), descriptor.getCharacteristic().getUuid().toString(), subscribed);
                } catch (UnsatisfiedLinkError e) {
                    Log.w(TAG, "Native onSubscriptionChange not linked: " + e.getMessage());
                }
            }
        }
    };

    @SuppressLint("MissingPermission")
    public static int startAdvertising(Context context, String localName, String serviceUuidsCsv) {
        if (context == null) return -1;

        if (bluetoothManager == null) {
            bluetoothManager = (BluetoothManager) context.getSystemService(Context.BLUETOOTH_SERVICE);
        }
        if (bluetoothManager == null) return -1;

        bluetoothAdapter = bluetoothManager.getAdapter();
        if (bluetoothAdapter == null || !bluetoothAdapter.isEnabled()) return -1;

        advertiser = bluetoothAdapter.getBluetoothLeAdvertiser();
        if (advertiser == null) {
            Log.e(TAG, "Device does not support BLE Advertising.");
            return -1;
        }

        if (gattServer == null) {
            gattServer = bluetoothManager.openGattServer(context, gattServerCallback);
            if (gattServer == null) {
                Log.e(TAG, "Failed to open GATT Server.");
                return -1;
            }
            for (BluetoothGattService service : pendingServices.values()) {
                gattServer.addService(service);
            }
        }

        AdvertiseSettings settings = new AdvertiseSettings.Builder()
                .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
                .setConnectable(true)
                .setTimeout(0)
                .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
                .build();

        AdvertiseData.Builder dataBuilder = new AdvertiseData.Builder()
                .setIncludeDeviceName(true);

        if (serviceUuidsCsv != null && !serviceUuidsCsv.isEmpty()) {
            for (String uuidStr : serviceUuidsCsv.split(",")) {
                String trimmed = uuidStr.trim();
                if (!trimmed.isEmpty()) {
                    try {
                        dataBuilder.addServiceUuid(new ParcelUuid(UUID.fromString(trimmed)));
                    } catch (Exception ignored) {}
                }
            }
        }

        advertiser.startAdvertising(settings, dataBuilder.build(), advertiseCallback);
        isAdvertising = true;
        return 0;
    }

    @SuppressLint("MissingPermission")
    public static int stopAdvertising() {
        if (advertiser != null && isAdvertising) {
            try {
                advertiser.stopAdvertising(advertiseCallback);
            } catch (Exception ignored) {}
            isAdvertising = false;
        }
        return 0;
    }

    public static int isAdvertising() {
        return isAdvertising ? 1 : 0;
    }

    public static int addService(String serviceUuidStr, boolean primary) {
        try {
            UUID uuid = UUID.fromString(serviceUuidStr);
            if (!pendingServices.containsKey(uuid)) {
                int serviceType = primary ? BluetoothGattService.SERVICE_TYPE_PRIMARY : BluetoothGattService.SERVICE_TYPE_SECONDARY;
                BluetoothGattService service = new BluetoothGattService(uuid, serviceType);
                pendingServices.put(uuid, service);
                if (gattServer != null) {
                    gattServer.addService(service);
                }
            }
            return 0;
        } catch (Exception e) {
            Log.e(TAG, "Failed to add service: " + e.getMessage());
            return -1;
        }
    }

    public static int addCharacteristic(String serviceUuidStr, String charUuidStr,
                                        boolean canRead, boolean canWrite, boolean canNotify) {
        try {
            UUID sUuid = UUID.fromString(serviceUuidStr);
            UUID cUuid = UUID.fromString(charUuidStr);

            BluetoothGattService service = pendingServices.get(sUuid);
            if (service == null) {
                service = new BluetoothGattService(sUuid, BluetoothGattService.SERVICE_TYPE_PRIMARY);
                pendingServices.put(sUuid, service);
                if (gattServer != null) {
                    gattServer.addService(service);
                }
            }

            int properties = 0;
            int permissions = 0;
            if (canRead) {
                properties |= BluetoothGattCharacteristic.PROPERTY_READ;
                permissions |= BluetoothGattCharacteristic.PERMISSION_READ;
            }
            if (canWrite) {
                properties |= BluetoothGattCharacteristic.PROPERTY_WRITE | BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE;
                permissions |= BluetoothGattCharacteristic.PERMISSION_WRITE;
            }
            if (canNotify) {
                properties |= BluetoothGattCharacteristic.PROPERTY_NOTIFY;
            }

            BluetoothGattCharacteristic characteristic = new BluetoothGattCharacteristic(cUuid, properties, permissions);

            if (canNotify) {
                BluetoothGattDescriptor cccd = new BluetoothGattDescriptor(
                        CCCD_UUID,
                        BluetoothGattDescriptor.PERMISSION_READ | BluetoothGattDescriptor.PERMISSION_WRITE
                );
                characteristic.addDescriptor(cccd);
            }

            service.addCharacteristic(characteristic);
            characteristicsMap.put(cUuid, characteristic);
            return 0;
        } catch (Exception e) {
            Log.e(TAG, "Failed to add characteristic: " + e.getMessage());
            return -1;
        }
    }

    @SuppressLint("MissingPermission")
    public static int updateCharacteristic(String charUuidStr, byte[] data) {
        try {
            UUID cUuid = UUID.fromString(charUuidStr);
            BluetoothGattCharacteristic characteristic = characteristicsMap.get(cUuid);
            if (characteristic == null || gattServer == null) return -1;

            characteristic.setValue(data != null ? data : new byte[0]);

            synchronized (connectedDevices) {
                for (BluetoothDevice device : connectedDevices) {
                    gattServer.notifyCharacteristicChanged(device, characteristic, false);
                }
            }
            return 0;
        } catch (Exception e) {
            Log.e(TAG, "Failed to update characteristic: " + e.getMessage());
            return -1;
        }
    }
}
