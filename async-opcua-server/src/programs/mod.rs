//! Programs (Part 10) module.
//! Implements state machines, async engines, and OPC UA methods to manage programs.

/// Program execution engine using tokio background tasks.
pub mod engine;
/// Program method call wrappers and registration helpers.
pub mod methods;
/// Program state machine types and transition logic.
pub mod state;

pub use engine::ProgramEngine;
pub use methods::register_program;
#[cfg(feature = "method-call")]
pub use methods::ProgramMethodHandler;
pub use state::{ProgramState, ProgramStateMachine};
