//! Firmware-over-the-air file transfer helpers.

/// Session lifecycle cleanup for temporary FOTA resources.
pub mod cleanup;
/// Real Open/Read/Write/Close/GetPosition/SetPosition behavior for a FileType object.
pub mod file_access;
/// Session-bound temporary FileType node creation.
pub mod file_node;
