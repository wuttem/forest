# IoT Forest

micro-platform for IoT Devices. MQTT Broker with integrated DeviceRegistry, DeviceShadow & TimeSeriesDatabase.

## Architecture Guidelines & Clarifications

> [!IMPORTANT]
> **User Management refers explicitly to Devices (Things).** 
> Forest is a platform strictly concerned with IoT devices. It currently does *not* manage authentication, permissions, or user accounts for human web users operating the platform. All tenant architecture, authentication, certificates, and username/password pairs exist solely to secure device connectivity to the `rumqttd` broker.

## Documentation

- [Agent Instructions](AGENT_INSTRUCTIONS.md)
- [Architecture Overview](docs/architecture.md)
- [Device & Tenant Management](docs/device_management.md)
- [Telemetry & Timeseries](docs/telemetry.md)