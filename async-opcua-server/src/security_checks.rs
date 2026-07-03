//! Centralized security check registry providing a bounded, queryable audit trail
//! of security-relevant server events. Implements the audit log concept from
//! OPC 10000-4 §6.5.1 (method 1: storage location) as a complement to the
//! existing event-based audit dispatch (method 2).

use std::{collections::VecDeque, fmt};

use opcua_types::{DateTime, StatusCode};

/// Category of a security-relevant validation performed by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityCheckCategory {
    /// Client application-instance certificate trust/expiry/revocation validation.
    CertificateValidation,
    /// User identity token (UserName, X509, IssuedToken) verification.
    UserAuthentication,
    /// SecureChannel security policy and mode negotiation.
    ChannelNegotiation,
    /// Role-based access control decision.
    RbacDecision,
}

impl fmt::Display for SecurityCheckCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecurityCheckCategory::CertificateValidation => write!(f, "CertificateValidation"),
            SecurityCheckCategory::UserAuthentication => write!(f, "UserAuthentication"),
            SecurityCheckCategory::ChannelNegotiation => write!(f, "ChannelNegotiation"),
            SecurityCheckCategory::RbacDecision => write!(f, "RbacDecision"),
        }
    }
}

/// Outcome of a single security check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityCheckOutcome {
    /// The check passed.
    Pass,
    /// The check failed.
    Fail,
}

/// A single entry in the security check registry.
#[derive(Debug, Clone)]
pub struct SecurityCheckEntry {
    /// When the check was performed.
    pub timestamp: DateTime,
    /// What was being validated.
    pub category: SecurityCheckCategory,
    /// Whether the check passed or failed.
    pub outcome: SecurityCheckOutcome,
    /// Status code for the result (`Good` on pass, error code on fail).
    pub reason: StatusCode,
    /// Affected client or user identifier.
    pub identity: String,
}

/// Bounded ring buffer of [`SecurityCheckEntry`] values, exposed through
/// [`ServerHandle`](crate::ServerHandle). Entries are evicted oldest-first
/// when the configured maximum count is reached.
#[derive(Debug, Clone)]
pub(crate) struct SecurityCheckRegistry {
    entries: VecDeque<SecurityCheckEntry>,
    max_entries: usize,
}

impl SecurityCheckRegistry {
    /// Create a new registry with the given maximum entry count.
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
        }
    }

    /// Record a security check outcome. Evicts the oldest entry if the
    /// registry is at capacity.
    pub(crate) fn record(
        &mut self,
        category: SecurityCheckCategory,
        outcome: SecurityCheckOutcome,
        reason: StatusCode,
        identity: impl Into<String>,
    ) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(SecurityCheckEntry {
            timestamp: DateTime::now(),
            category,
            outcome,
            reason,
            identity: identity.into(),
        });
    }

    /// Convenience: record a pass event.
    pub(crate) fn record_pass(
        &mut self,
        category: SecurityCheckCategory,
        identity: impl Into<String>,
    ) {
        self.record(
            category,
            SecurityCheckOutcome::Pass,
            StatusCode::Good,
            identity,
        )
    }

    /// Convenience: record a fail event.
    pub(crate) fn record_fail(
        &mut self,
        category: SecurityCheckCategory,
        reason: StatusCode,
        identity: impl Into<String>,
    ) {
        self.record(category, SecurityCheckOutcome::Fail, reason, identity)
    }

    /// Return a snapshot of all entries currently in the registry.
    pub(crate) fn snapshot(&self) -> Vec<SecurityCheckEntry> {
        self.entries.iter().cloned().collect()
    }

    /// Return the number of entries currently in the registry.
    pub(crate) fn count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for SecurityCheckRegistry {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_retrieves_entries() {
        let mut registry = SecurityCheckRegistry::new(100);
        registry.record(
            SecurityCheckCategory::CertificateValidation,
            SecurityCheckOutcome::Fail,
            StatusCode::BadCertificateUntrusted,
            "urn:test-client",
        );
        registry.record(
            SecurityCheckCategory::UserAuthentication,
            SecurityCheckOutcome::Pass,
            StatusCode::Good,
            "test-user",
        );

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(
            snapshot[0].category,
            SecurityCheckCategory::CertificateValidation
        );
        assert_eq!(snapshot[0].outcome, SecurityCheckOutcome::Fail);
        assert_eq!(
            snapshot[1].category,
            SecurityCheckCategory::UserAuthentication
        );
        assert_eq!(snapshot[1].outcome, SecurityCheckOutcome::Pass);
    }

    #[test]
    fn bounded_registry_evicts_oldest() {
        let mut registry = SecurityCheckRegistry::new(10);
        for i in 0..15 {
            registry.record(
                SecurityCheckCategory::ChannelNegotiation,
                SecurityCheckOutcome::Pass,
                StatusCode::Good,
                format!("client-{i}"),
            );
        }
        assert_eq!(registry.count(), 10);
        let snapshot = registry.snapshot();
        assert_eq!(snapshot[0].identity, "client-5");
        assert_eq!(snapshot[9].identity, "client-14");
    }
}
