# Configuration & Running

Forest is designed to be simple and straightforward to configure and deploy, whether locally for development or in a production environment.

## Running the System

To run the Forest platform, you can compile and start the server using standard Cargo commands. Make sure you have a working Rust toolchain installed:

```bash
cargo run --release
```

## System Configuration

Forest reads its startup variables using an internal configuration system. This dictates how the `rumqttd` broker binds its ports, how the HTTP API initializes, and how data is stored.

By default, the MQTT broker exposes two primary listener ports:
- **1883**: Standard MQTT over TCP.
- **8884**: MQTT over TLS (requires proper CA certificates to be configured).

The REST API typically listens on a designated local port (by default `8807`), which serves all external management, certificate generation, and timeseries endpoints.

### Configuration Format

Configurations are defined through a JSON file structure (or built programmatically). It includes specifications for the database, processor mappings, and MQTT behavior:

```json
{
  "database": {
    "path": "sqlite://.forest.db?mode=rwc",
    "timeseries_path": "postgres://user:password@localhost/timeseries_db"
  },
  "processor": {
    "shadow_topic_prefix": "things/",
    "telemetry_topics": ["things/+/data"]
  }
}
```

This flexibility allows you to easily point the timeseries blob storage to a distributed `TimescaleDB` PostgreSQL instance while maintaining device configuration on a local `SQLite` file, or adjust the default topic namespaces your devices publish metric data to.
