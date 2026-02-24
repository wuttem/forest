# MQTT Broker

The foundational communication layer of the Forest IoT Platform relies on a high-concurrency [MQTT](https://mqtt.org/) (Message Queuing Telemetry Transport) server built with `rumqttd`.

## Real-Time Messaging

MQTT is the standard transport option for the core features of the Forest ecosystem. Both **Device Shadows** and **Telemetry** data are primarily driven by MQTT publishers and subscribers. The MQTT protocol provides lightweight, low-bandwidth bidirectional connectivity, ideal for hardware devices.

- **Bidirectional Capabilities:** Unlike the stateless nature of REST, an MQTT connection is persistently kept alive and bi-directional. This allows the broker to stream shadow deltas and routing configuration exactly as state changes without polling.
- **Robust Routing:** Employs industry standard topic routing. (e.g., `things/{device_id}/shadow/update`).
- **TLS Security (mTLS):** Supports highly secured inbound connections via the 8883 port binding for natively verified client x.509 certificates.

## Interoperability with REST

It's critically important to note that **there are two equivalent transport options** connected directly into the Forest engine:
1. This MQTT Broker
2. The [REST API](rest_api.md)

They are designed to interoperate seamlessly. A device can publish its state using an MQTT message (`things/sensor_1/shadow/update`), and a web dashboard can query that exact state synchronously directly via an HTTP GET to the REST API (`/default/shadow/sensor_1`).

Because Forest acts as a unified platform, the core engine intercepts both transports equally, allowing developers full flexibility depending on their networking restrictions.
