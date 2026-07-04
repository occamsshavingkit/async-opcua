# TODO

Ideas that could be implemented.

## Remaining

- Flesh out the server and client SDK with tooling for ease of use.
  - Make it even easier to implement custom node managers.

## Deferred integration tests (feature 055)

- RSA-KEM encrypted UserName token integration test: needs a full client+server setup with RSA certificates and two-phase secure client connect.
- Embedded profile secure channel smoke test (feature 054, `#[ignore]`d): needs two-phase client connect.
- Standard profile X509/RegisterServer2 tests (feature 054, `#[ignore]`d): need in-process LDS peer.

## Done

- ~~Add Nano/Micro/Embedded conformance-profile builds~~ — feature 054.
- ~~Encrypted identity-token secrets: RSA-DH / authenticated-encryption variants~~ — feature 055.
- ~~Implement a better framework for security checks~~ — feature 055.
- ~~Write a sophisticated server example with a persistent store~~ — `samples/persistent-store/`.
- ~~Write some "bad ideas" servers~~ — feature 057 (chat, chaos, filesystem bridge, reverse bridge).
- ~~Write a framework for method calls~~ — `method_typed.rs`.
- ~~Implement `Query`~~ — QueryFirst/QueryNext + client API.
- ~~Make `async-opcua-pubsub` and `async-opcua-history-sqlite` optional facade deps~~ — feature 055.
- ~~Complexity cuts (Big-O)~~ — features 056/057.
