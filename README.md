# IoT Forest

Forest is a lightweight, all-in-one micro-platform for managing Internet of Things (IoT) devices. It acts as the backbone for connecting, securing, and maintaining the state and timeseries data of distributed edge devices.

## Core Components

Forest integrates major pillars of an IoT ecosystem into a single binary. It is important to note that **the core features for devices operates transparently across two primary transport options: MQTT and the REST API**. The backend unifies them so developers can interact with devices via either protocol simultaneously.

1. **[MQTT Broker](docs/mqtt_broker.md)**
   Built securely on `rumqttd`, providing lightweight, bidirectional telemetry and state transmission natively to millions of distributed hardware devices.

2. **[REST API](docs/rest_api.md)**
   A comprehensive HTTP interface providing a synchronous transport option for dashboards and backend services to fetch telemetry, mutate device shadows, or script administrative actions seamlessly.

3. **[Device & Tenant Management](docs/device_management.md)**
   A multi-tenant registry for provisioning hardware and organizing them automatically. Provides robust connection authentication securely locking down the MQTT endpoints.

4. **[Device Shadow](docs/device_shadow.md)**
   Allows the platform and devices to sync state via JSON documents. Even when devices go offline, applications can query and modify their "shadow" state via the REST API or MQTT, syncing seamlessly upon reconnection.

5. **[Telemetry](docs/telemetry.md)**
   Automatically scrapes defined metrics and stores numerical/dimensional data to a high-performance timeseries database, allowing analytics over historical values without extra pipelines.

## General Information

- [Configuration & Running](docs/configuration_and_run.md)
- [Agent Instructions](AGENT_INSTRUCTIONS.md)

> [!IMPORTANT]
> **User Management refers explicitly to Devices (Things).** 
> Forest is a platform strictly concerned with IoT devices. It currently does *not* manage authentication, permissions, or user accounts for human web users operating the platform. All tenant architecture, authentication, certificates, and username/password pairs exist solely to secure device connectivity to the broker.