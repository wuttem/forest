# Forest IoT Platform Architecture

Forest is a lightweight, all-in-one platform for managing Internet of Things (IoT) devices. It acts as the backbone for connecting, securing, and maintaining the state and timeseries data of distributed edge devices.

## Core Capabilities

Forest integrates four major pillars of an IoT ecosystem into a single binary:

1. **High-Performance MQTT Broker**: Built on `rumqttd`, Forest natively brokers MQTT connections via TCP and TLS, ensuring scalable packet delivery, message routing, and reliable subscriptions.
2. **Device Registry (Multi-Tenant)**: Acts as the source of truth for knowing which devices exist, what metadata they carry, and their connectivity status. 
3. **Device Shadows**: Allows the platform and devices to sync state via JSON documents (`$aws/things/...`). Even when devices go offline, applications can query and modify their "shadow" state, syncing upon reconnection.
4. **Time-Series Database**: Automatically scrapes defined MQTT metrics and stores numerical/dimensional data to a high-performance Timeseries DB, allowing analytics over historical values without extra pipelines.

## Component Overview

### The Broker & API
- **MQTT Server**: Bound by default to `1883` (TCP) and/or `8883` (TLS). Forest wraps the broker and injects a dynamic authentication handler.
- **REST API (`axum`)**: Provides standard HTTP endpoints to configure device shadows, upload certificates, provision tenants, manage data configs, and fetch timeseries data.

### Storage (`SQLite/sqlx`)
Forest uses an asynchronous `SQLite` backing store (`sqlite://forest.db`), containing tables for:
*   Tenants and their Authentication Configurations
*   Device Metadata, passwords, and registry info
*   Device Shadows (State Documents)
*   Time-Series blobs (compressed binary representations for rapid querying)

## Learn More

- For details on setting up Tenants, Authenticating devices, and managing CAs: see [Device & Tenant Management](device_management.md).
- Want to integrate the Server? Review our extensive [Rust Examples](../examples).
