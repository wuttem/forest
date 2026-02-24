# REST API

In conjunction with the native MQTT interface, Forest exposes a comprehensive **RESTful HTTP API**. This provides the second parallel transport option for integrating and controlling the Forest IoT platform.

## A Unified Command Interface

While MQTT excels at continuous data streaming for edge devices, the REST API provides synchronous request-response semantics ideal for web applications, backends, and administrative tasks.

Crucially, **the core features of the platform operate transparently across both transports.** 
- You can query or update **Device Shadows** using standard `GET` and `POST` requests instead of waiting for MQTT responses.
- You can push **Telemetry** directly via HTTP if your devices are unable to keep a permanent MQTT connection open. 

## Administrative Tasks

The REST API is exclusively responsible for administrative actions:
- **Tenant Management:** Provisioning new organizational tenants (`POST /tenants`).
- **Device Provisioning:** Hashing passwords or generating mTLS Client Certificates for newly manufactured hardware.
- **Data Configuration:** Registering schema pointers to extract nested metrics from raw JSON payload (`PUT /default/dataconfig/...`).

Both transports share the singular internal state maintained by the SQLite backing database—ensuring perfect synchrony regardless of which path data takes.
