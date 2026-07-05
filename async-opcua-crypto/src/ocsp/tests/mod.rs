//! OCSP test modules.
//!
//! Authored separately from the production implementation so that the
//! responder (built in feature 058 US1) is never validated solely by tests
//! written by its own author.

mod ocsp_responder;
