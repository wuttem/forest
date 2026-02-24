# Time Synchronization

Forest provides an integrated time server mechanism for IoT devices. This allows devices to retrieve the current server time and implement simple latency compensation to maintain accurate clocks. Reliable time synchronization is critical for tasks like accurate data timestamping, security token validation, and synchronized actions.

Forest supports time synchronization over both REST APIs and MQTT, ensuring compatibility with a wide range of devices.

## How Time Compensation Works

Devices often have inaccurate or drifting internal clocks, and retrieving time over a network introduces latency. Forest's time synchronization mechanism uses a simple process to compensate for this latency, assuming symmetric network delays.

1.  **Request Time (`T0`)**: The device records its current internal time (`T0`) and sends it to the server in a time request.
2.  **Server Processing (`T1`)**: The server receives the request, records its current accurate time (`T1`), and sends `T1` back to the device along with the device's original time (`T0`).
3.  **Receive Response (`T2`)**: The device receives the response and records its current internal time (`T2`).
4.  **Calculate Network Delay**: The total round-trip time is `T2 - T0`. Assuming symmetric latency, the one-way network delay is `(T2 - T0) / 2`.
5.  **Calculate Accurate Time**: The server's time at `T2` (now) can be estimated as `T1 + (T2 - T0) / 2`. The device can then adjust its clock or calculate an offset (`Offset = T1 + (T2 - T0) / 2 - T2 = T1 - (T0 + T2) / 2`).

## Method 1: MQTT Time Sync

Forest uses a specific topic structure for MQTT time requests. This is the preferred method for devices already maintaining an active MQTT connection.

### Topic Structure

*   **Request Topic**: `{shadow_topic_prefix}{device_id}/time/request` (e.g., `things/my-device/time/request`)
*   **Response Topic**: `{shadow_topic_prefix}{device_id}/time/response` (e.g., `things/my-device/time/response`)

### Example Interaction

1.  **Device Setup**: The device subscribes to `things/my-device/time/response`.
2.  **Device Request**: The device publishes a JSON payload to `things/my-device/time/request` containing its current time (`T0`).
    ```json
    {
      "device_time": 1715000000000
    }
    ```
    *(Note: The `device_time` is optional. If omitted, latency compensation cannot be accurately performed.)*
3.  **Server Response**: The server replies on `things/my-device/time/response` with its current time (`server_time`) and the device's requested time (`device_time`).
    ```json
    {
      "server_time": 1715000000100,
      "device_time": 1715000000000
    }
    ```
4.  **Device Calculation**: The device applies the latency compensation logic described above.

## Method 2: REST API Time Sync

Devices can also perform time synchronization using a standard HTTP API endpoint. This is useful for initial provisioning or devices without MQTT access.

### Endpoint Details

*   **URL**: `GET /time`
*   **Query Parameters**:
    *   `device_time` (optional): The device's current time (`T0`) in milliseconds since the Unix epoch.

### Example Interaction

1.  **Device Request**: The device makes an HTTP GET request to `/time?device_time=1715000000000`.
    ```bash
    curl "http://<forest-server-address>/time?device_time=1715000000000"
    ```
2.  **Server Response**: The server returns a 200 OK status with a JSON payload.
    ```json
    {
      "server_time": 1715000000100,
      "device_time": 1715000000000
    }
    ```
    *(Note: If the `device_time` parameter is not provided in the request, it will be omitted from the response.)*
3.  **Device Calculation**: The device applies the latency compensation logic.
