use std::{collections::VecDeque, fmt};

use opcua_types::{DateTime, StatusCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityCheckCategory {
    CertificateValidation,
    UserAuthentication,
    ChannelNegotiation,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityCheckOutcome {
    Pass,
    Fail,
}

#[derive(Debug, Clone)]
pub struct SecurityCheckEntry {
    pub timestamp: DateTime,
    pub category: SecurityCheckCategory,
    pub outcome: SecurityCheckOutcome,
    pub reason: StatusCode,
    pub identity: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SecurityCheckRegistry {
    entries: VecDeque<SecurityCheckEntry>,
    max_entries: usize,
}

impl SecurityCheckRegistry {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
        }
    }

    pub fn record(
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
    pub fn record_pass(&mut self, category: SecurityCheckCategory, identity: impl Into<String>) {
        self.record(
            category,
            SecurityCheckOutcome::Pass,
            StatusCode::Good,
            identity,
        )
    }

    /// Convenience: record a fail event.
    pub fn record_fail(
        &mut self,
        category: SecurityCheckCategory,
        reason: StatusCode,
        identity: impl Into<String>,
    ) {
        self.record(category, SecurityCheckOutcome::Fail, reason, identity)
    }

    pub fn snapshot(&self) -> Vec<SecurityCheckEntry> {
        self.entries.iter().cloned().collect()
    }

    pub fn count(&self) -> usize {
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
