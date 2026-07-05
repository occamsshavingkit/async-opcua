mod metadata;
mod tags;

pub use metadata::MetadataNodeManagerBuilder;
pub use metadata::CURRENT_TICK;
pub use tags::TagNodeManagerBuilder;

// QuickNodeManager example
//
// For simpler use-cases, `QuickNodeManager` provides a builder API that reduces
// boilerplate. Instead of implementing `InMemoryNodeManagerImplBuilder`, you
// chain `.variable()` and `.object()` calls:
//
// ```ignore
// use opcua::server::node_manager::builder::QuickNodeManager;
//
// fn make_my_node_manager() -> QuickNodeManager {
//     QuickNodeManager::new("urn:my-namespace")
//         .variable("speed", 0.0f64).writable().add()
//         .object("motor")
//             .add_variable("rpm", 0u32).add()
//             .add()
// }
// ```
//
// For dynamic values, attach read/write callbacks to any variable:
//
// ```ignore
// .variable("temperature", 25.0f64)
//     .read_callback(|_ctx| Ok(DataValue::new_now(42.0f64)))
//     .add()
// ```
//
// See `async_opcua_server::node_manager::builder` for the full API.
