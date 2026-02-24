# MQTT Broker & Device Registry

Forest is designed to support **multi-tenant** architectures securely. It does not manage human web users; instead, its authentication system is built explicitly to identify, provision, and secure *Devices* connecting to the MQTT broker.

There are three key entities in Forest:
1. **Server (Platform)**: The operational layer hosting the `rumqttd` broker.
2. **Tenants (Organizations)**: Logical groups of devices operating under a shared namespace and isolation framework (e.g., an enterprise customer).
3. **Devices (Things)**: The edge hardware connecting to Forest, configured under a specific Tenant.

## The Tenant Setup

Before any device can connect to the broker, its parent **Tenant** must be provisioned.

### 1. Creating a Tenant
Tenants are registered through the REST API, taking an `AuthConfig` which dictates how devices inside this tenant can authenticate.

```json
// POST /tenants
{
  "tenant_id": "home_automation_inc",
  "auth_config": {
    "allow_passwords": true,
    "allow_certificates": true
  }
}
```

## Device Authentication Strategies

Devices connecting to the broker must supply credentials that map up seamlessly to their parent tenant configuration. Forest supports two parallel authentication channels:

### Strategy 1: Username & Password
If `allow_passwords` is true, devices can connect using a standard Username/Password pair.

1. Using the REST API, an admin hashes a new password for the device:
   `POST /{tenant_id}/devices/{device_id}/passwords` with `{"username": "sensor_01", "password_plaintext": "secret"}` (Forest bcrypts this safely on the backend).
2. The device connects to port 1883/8883, supplying:
   - Client ID: `{device_id}`
   - Username: `{tenant_id}`
   - Password: `secret`

### Strategy 2: Client Certificates (mTLS)
If `allow_certificates` is true, devices authenticate using specialized x.509 Client Certificates. This process represents a far more robust standard for zero-trust provisioning. 

#### Managing Certificate Authorities (CAs)
Forest supports automatic generation or manual uploading of custom Certificate Authorities per-tenant.

- **Server-wide CA:** Forest maintains a default global CA (`cacerts/ca.pem`) that can sign certs across the server. Use `POST /cacert/server` to generate it.
- **Tenant-specific CAs:** Tenants who wish to bring their own PKI security can do so! 
  - Send a PEM string to `POST /tenants/{tenant_id}/cacert` to upload an organizational CA, ensuring the platform accepts natively-signed hardware.
  - Or, generate an isolated CA right on Forest using `POST /tenants/{tenant_id}/cacert/generate`. 

*(Note: Forest prevents catastrophic lockouts. If an admin pushes a bad CA via the upload API, Forest saves a `.pem.bak` backup automatically).*

#### Provisioning Device Certificates
Provisioning a device securely into the registry is a single API call for automated systems:

`POST /tenants/{tenant_id}/devices/{device_id}/client_cert/generate`

This endpoint seamlessly queries the Tenant's CA and securely issues a robust RSA-2048 x.509 Certificate and Private Key bundle constrained to the requested Device ID. The device then connects via mTLS supplying its client certificate. Forest validates the chain against the respective Tenant's CA, extracts the Common Name (mapping it to the Tenant ID), and allows the connection dynamically.

### Device Rate Limiting

Forest uses global **Dynamic Rate Limits** (messages/minute) enforced automatically by the broker. When a network route experiences widespread congestion, the broker mathematically tracks histograms and drops the top-publishing devices exceeding their safe designated thresholds, thereby protecting link stability.

Forest uses the following safe global defaults per connection:
- `lower_rate`: **60 msgs/minute** (probation safety bounds)
- `higher_rate`: **6000 msgs/minute** (forced disconnect thresholds)

You can view live histograms of these rates spanning the past 5 minutes natively through the device API:

```bash
GET /{tenant_id}/devices/{device_id}
```

The response includes a `past_minute_rates` array. This array contains up to 5 metrics tracking the historic minute `timestamp` and `mqtt_message_rate_in` (the exact counted volume of messages traversing the router for each of the past 5 minutes natively).

---

## Examples & Walkthroughs

If you'd like to integrate User & Tenant Management via Rust, check out the provided [`auth_admin.rs` example](../examples/auth_admin.rs). It demonstrates spinning up an Ephemeral Forest Platform instance with custom Certificate Directories in memory, binding API routines locally, and configuring connection handling configurations safely across custom ports.
