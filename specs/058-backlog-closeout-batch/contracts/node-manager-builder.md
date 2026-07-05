# Contract: Node Manager Builder API

## Module

`async-opcua-server::node_manager::quick`

## Public API

### Types

```rust
/// A high-level builder for creating a custom node manager with minimal boilerplate.
///
/// Wraps `InMemoryNodeManager` internally and implements `NodeManagerBuilder`.
///
/// # Example
///
/// ```ignore
/// let nm = QuickNodeManager::new("urn:my-namespace")
///     .variable("Counter", 0u32)
///         .writable()
///         .add()
///     .variable("Temperature", 25.0f64)
///         .add()
///     .build(context);
/// ```
pub struct QuickNodeManager {
    namespace_uri: String,
    variables: Vec<PendingVariable>,
    objects: Vec<PendingObject>,
}

/// A variable being defined in a `QuickNodeManager`.
pub struct PendingVariable {
    name: String,
    initial_value: Variant,
    writable: bool,
    data_type: Option<NodeId>,
    read_cb: Option<Box<dyn Fn(&ReadContext) -> Result<DataValue, StatusCode> + Send + Sync>>,
    write_cb: Option<Box<dyn Fn(&WriteContext, Variant) -> Result<(), StatusCode> + Send + Sync>>,
}

/// A pending object definition (folder grouping variables).
pub struct PendingObject {
    name: String,
    type_definition: NodeId,
    children: Vec<PendingVariable>,
}
```

### Builder Methods

```rust
impl QuickNodeManager {
    /// Create a new builder for the given namespace URI.
    pub fn new(namespace_uri: &str) -> Self;

    /// Add a variable definition. Returns a `VariableBuilder` for chaining.
    ///
    /// `initial_value` determines the data type (inferred from the Variant).
    pub fn variable<V: Into<Variant>>(mut self, name: &str, initial_value: V) -> VariableBuilder<Self>;

    /// Add an object (folder) with children.
    pub fn object(mut self, name: &str) -> ObjectBuilder<Self>;
}

impl NodeManagerBuilder for QuickNodeManager {
    fn build(self: Box<Self>, context: ServerContext) -> Arc<DynNodeManager>;
}

/// A variable builder returned by `QuickNodeManager::variable()`.
/// Allows setting writable flag and callbacks before calling `.add()`.
pub struct VariableBuilder<P> {
    parent: P,
    var: PendingVariable,
}

impl<P> VariableBuilder<P> {
    /// Make the variable writable.
    pub fn writable(mut self) -> Self;

    /// Set a custom read callback.
    pub fn read_callback<F>(mut self, cb: F) -> Self
    where F: Fn(&ReadContext) -> Result<DataValue, StatusCode> + Send + Sync + 'static;

    /// Set a custom write callback.
    pub fn write_callback<F>(mut self, cb: F) -> Self
    where F: Fn(&WriteContext, Variant) -> Result<(), StatusCode> + Send + Sync + 'static;

    /// Finalize the variable definition and return to the parent builder.
    pub fn add(self) -> P;
}
```

### Registration with Server

```rust
// Usage:
let qnm = QuickNodeManager::new("urn:my-ns")
    .variable("Status", "OK")
        .read_callback(|ctx| Ok(DataValue::new_now(Variant::String("RUNNING".into()))))
        .add()
    .variable("Count", 0u32)
        .writable()
        .add();

// Register via the standard ServerBuilder API:
let server = ServerBuilder::new()
    .with_node_manager(qnm)
    .build()?;
```

### Compatibility

- `QuickNodeManager` implements `NodeManagerBuilder`, so it's compatible with `ServerBuilder::with_node_manager()`
- Does not modify any existing trait or type
- Existing `InMemoryNodeManager` and custom `NodeManager` trait implementations continue to work unchanged (FR-009)
- The builder delegates to `InMemoryNodeManager` internally; advanced use cases can still access the underlying address space

### Non-goals

- Method callbacks via the builder (advanced users use the raw trait)
- Event/Alarm support via the builder
- History read/write via the builder
- Automatic NodeSet2 XML generation
