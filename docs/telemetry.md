# Telemetry Data Ingestion

Forest enables you to ingest, store, and dynamically query high-frequency timeseries data.

## 1. Timeseries Configuration
Forest decouples State storage from Timeseries storage, allowing you to run Forest on an embedded SQLite database while piping huge amounts of telemetry straight to a powerful TimescaleDB backend.

In your `config.json`:
```json
"database": {
    "path": "sqlite://.forest.db?mode=rwc",
    "timeseries_path": "postgres://user:password@localhost/timeseries_db"
}
```
If `timeseries_path` is not provided, Timeseries data will be stored in the primary database.
**Note**: If the `timeseries_path` is a Postgres URI, Forest will automatically attempt to enable the `timescaledb` extension and initialize a Hypertable.

## 2. Dynamic Metric Extraction (DataConfig)
Forest requires explicit instructions on which values from a JSON payload to extract and store. You define these using **DataConfigs** on either a Tenant level or a Device-Prefix level.

### Creating rules via HTTP
To extract `temperature` (float) and `humidity` (integer) from `{"temp": 22.4, "hum": 65}` sent by device `sensor_1`:
```bash
curl -X PUT http://localhost:8807/default/dataconfig/device/sensor_
-d '{
    "metrics": [
        {
            "name": "temperature",
            "json_pointer": "/temp",
            "data_type": "Float"
        },
        {
            "name": "humidity",
            "json_pointer": "/hum",
            "data_type": "Int"
        }
    ]
}'
```
Any JSON message matching `sensor_*` will now be parsed according to these rules.

## 3. Ingestion Methods

### A: HTTP API (REST)
Simply send an HTTP POST request to the device's endpoint.
```bash
curl -X POST http://localhost:8807/default/data/sensor_1 \
-H "Content-Type: application/json" \
-d '{"temp": 24.1, "hum": 40}'
```

### B: MQTT 
If the device publishes telemetry to the MQTT broker, the Forest Server processor will automatically intercept it. By default, the topic is `things/{device_id}/data`, but you can customize this globally.
```json
"processor": {
    "telemetry_topics": ["events/+/telemetry", "things/+/data"]
}
```

```bash
# E.g. publishing to MQTT using mosquitto_pub
mosquitto_pub -t 'things/sensor_1/data' -m '{"temp": 24.1, "hum": 40}' -u sensor_1 -P pass
```

## 4. Querying Metrics
Once stored, you can query a metric timeseries using the HTTP API:

**Get recent limits (e.g. last 5 values for temperature):**
```bash
curl http://localhost:8807/default/data/sensor_1/temperature/last?limit=5
```

**Output:**
```json
{
  "device_id": "sensor_1",
  "metric": "temperature",
  "data": [
    [1712211561, 22.4],
    [1712211572, 24.1]
  ]
}
```
