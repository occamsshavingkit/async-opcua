# Data Model: Backlog Closeout Batch

## US1 — OCSP Responder

### Entities

**CertStatusRecord**
- `serial_number: SerialNumber` — certificate serial number (from x509-cert)
- `status: CertStatusVariant` — Good, Revoked { revocation_time: DateTime, revocation_reason: Option<u32> }, or Unknown
- No relationships — standalone lookup table

**OcspResponderConfig**
- `signer_cert: X509` — the CA certificate whose key signs OCSP responses
- `signer_key: PrivateKey` — the CA private key for signing responses
- `this_update: DateTime` — when the response was generated (set at call time)
- `next_update: Option<DateTime>` — when the response expires (configurable validity window)
- `status_db: HashMap<Vec<u8>, CertStatusVariant>` — serial number → status mapping

**CertStatusVariant** (enum)
- `Good`
- `Revoked { revocation_time: DateTime, revocation_reason: Option<u32> }`
- `Unknown`

### State Transitions

CertStatusRecord transitions:
- `Good → Revoked`: Administrator updates the status database
- `Revoked → Good`: Should not happen (reversal is semantically invalid per PKI best practice), but the in-memory DB allows any update
- No automated transitions — the status database is manually managed by the caller

## US2 — SDK Node-Manager Tooling

### Entities

**QuickNodeManager** (new)
- `namespace_uri: String` — the namespace URI for this node manager
- `address_space: Arc<RwLock<AddressSpace>>` — delegates to InMemoryNodeManager internally
- `variables: Vec<VariableDef>` — pending variable definitions to register during build()
- `objects: Vec<ObjectDef>` — pending object definitions
- `read_callbacks: HashMap<NodeId, Box<dyn ReadCallback>>` — registered read callbacks
- `write_callbacks: HashMap<NodeId, Box<dyn WriteCallback>>` — registered write callbacks

**VariableDef** (new, internal)
- `name: String` — browse name
- `initial_value: Variant` — initial value
- `data_type: NodeId` — data type
- `writable: bool` — whether the variable accepts writes
- `read_callback: Option<ReadCallback>` — optional custom read logic
- `write_callback: Option<WriteCallback>` — optional custom write logic

**ObjectDef** (new, internal)
- `name: String` — browse name
- `type_definition: NodeId` — ObjectType node id
- `children: Vec<VariableDef>` — child variables

### No state transitions — builder is consumed on build(), producing an Arc<DynNodeManager>

## US3 — RSA-KEM Integration Test

No new persistent entities. The test uses existing types:
- `Tester` — creates server + client
- `IdentityToken::UserName` — with encrypted password using RSA-KEM
- `Session` — activated session for verifying the test

## US4 — Embedded Profile Secure Channel Test

No new entities. The test uses existing types:
- `EmbeddedTester` — spawns embedded server
- Two-phase connect: `client.get_endpoints()` → extract server cert → `connect_to_matching_endpoint` with cert

## US5 — Standard Profile X509/RegisterServer2 Tests

No new persistent entities. The test uses existing types:
- `StandardTester` — spawns standard server
- For X509: `IdentityToken::X509` with user certificate path
- For RegisterServer2: Second `Server` instance (LDS peer) with `discovery-mdns` feature
