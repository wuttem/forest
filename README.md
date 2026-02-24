# IoT Forest

Forest is a lightweight, all-in-one micro-platform for managing Internet of Things (IoT) devices. It acts as the backbone for connecting, securing, and maintaining the state and timeseries data of distributed edge devices.

## Core Components

Forest integrates major pillars of an IoT ecosystem into a single binary.

1. **[MQTT Broker & Registry](docs/mqtt_broker.md)**
   Built on `rumqttd`, Forest natively brokers MQTT connections via TCP and TLS. It includes a multi-tenant device registry and robust authentication (passwords, x.509 certificates).

2. **[Device Shadow](docs/device_shadow.md)**
   Allows the platform and devices to sync state via JSON documents. Even when devices go offline, applications can query and modify their "shadow" state, syncing seamlessly upon reconnection.

3. **[Telemetry](docs/telemetry.md)**
   Automatically scrapes defined MQTT metrics and stores numerical/dimensional data to a high-performance timeseries database, allowing analytics over historical values without extra pipelines.

## General Information

- [Configuration & Running](docs/configuration_and_run.md)
- [Agent Instructions](AGENT_INSTRUCTIONS.md)

> [!IMPORTANT]
> **User Management refers explicitly to Devices (Things).** 
> Forest is a platform strictly concerned with IoT devices. It currently does *not* manage authentication, permissions, or user accounts for human web users operating the platform. All tenant architecture, authentication, certificates, and username/password pairs exist solely to secure device connectivity to the broker.