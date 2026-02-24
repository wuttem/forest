# Device Shadow

The **Device Shadow** is an integral core component of the Forest IoT Platform. It provides a persistent JSON document used to store and retrieve the current state information for any device connected to the broker.

## Overview

The Device Shadow service maintains a device's state so that backend applications and other systems can read messages and interact with the device whether the device is currently online or offline. It bridges the gap between the asynchronous reality of hardware and the synchronous expectations of web backends.

## How it works

1. **Reporting State:** Devices publish their current hardware/sensor state via MQTT to a specific shadow topic (e.g., `things/{device_id}/shadow/update`).
2. **Persistence:** The Forest Processor intercepts these MQTT messages and updates the device's persistent state document in the database (recording the `reported` state).
3. **Requesting Changes:** A web application can update the `desired` state of a device using the REST API to request that the device turn on, change configuration, etc.
4. **Delta Calculation:** When the `desired` state differs from the `reported` state, the platform computes a "delta".
5. **Syncing:** This delta is instantly published back to the device via MQTT so that the device can apply the new state. Once applied, the device reports the new state back, closing the loop.

This flow ensures that no command is lost, and the state always eventually converges.
