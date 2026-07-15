//! CU-indexed coverage reporting for OPC UA Foundation profile snapshots.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

/// Normalized OPC UA Foundation profile snapshot fields used by the report.
#[derive(Debug, Deserialize)]
pub struct NormalizedSnapshot {
    canonical_profiles: BTreeMap<String, Profile>,
    conformance_units: Vec<ConformanceUnit>,
    relationships: Relationships,
}

/// Canonical profile metadata.
#[derive(Debug, Deserialize)]
pub struct Profile {
    display_name: String,
    opc_id: u32,
    opc_profile_uri: String,
}

/// Conformance Unit metadata.
#[derive(Debug, Deserialize)]
pub struct ConformanceUnit {
    name: String,
    opc_id: u32,
}

#[derive(Debug, Deserialize)]
struct Relationships {
    transitive_cu_closure: BTreeMap<String, Vec<u32>>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum EvidenceStatus {
    Implemented,
    Partial,
    Gap,
    NeedsProof,
    SourceIssue,
}

impl EvidenceStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::Partial => "partial",
            Self::Gap => "gap",
            Self::NeedsProof => "needs-proof",
            Self::SourceIssue => "source-issue",
        }
    }
}

/// Parses the normalized Foundation profile snapshot JSON.
pub fn parse_snapshot(input: &str) -> Result<NormalizedSnapshot, serde_json::Error> {
    serde_json::from_str(input)
}

/// Generates a conservative CU-indexed Markdown report for canonical profiles.
#[must_use]
pub fn generate_markdown_report(snapshot: &NormalizedSnapshot) -> String {
    let conformance_units = snapshot
        .conformance_units
        .iter()
        .map(|unit| (unit.opc_id, unit.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut output = String::from("# OPC UA Foundation CU Coverage Report\n\n");
    output.push_str("Status labels are evidence categories, not certification claims.\n\n");

    for tier in ["nano", "micro", "embedded", "standard"] {
        let Some(profile) = snapshot.canonical_profiles.get(tier) else {
            continue;
        };
        let closure_key = profile.opc_id.to_string();
        let closure = snapshot
            .relationships
            .transitive_cu_closure
            .get(&closure_key)
            .map(Vec::as_slice)
            .unwrap_or_default();

        output.push_str("## ");
        output.push_str(&profile.display_name);
        output.push_str("\n\n");
        output.push_str("- OPC profile id: `");
        output.push_str(&profile.opc_id.to_string());
        output.push_str("`\n");
        output.push_str("- URI: `");
        output.push_str(&profile.opc_profile_uri);
        output.push_str("`\n");
        output.push_str("- CU closure size: ");
        output.push_str(&closure.len().to_string());
        output.push_str("\n\n");
        output.push_str("| CU | Name | Status | Evidence |\n");
        output.push_str("|---:|---|---|---|\n");

        for cu_id in closure {
            let (name, status) = match conformance_units.get(cu_id) {
                Some(name) => (*name, classify_cu(*cu_id)),
                None => (
                    "Missing from normalized CU list",
                    EvidenceStatus::SourceIssue,
                ),
            };
            output.push_str("| ");
            output.push_str(&cu_id.to_string());
            output.push_str(" | ");
            output.push_str(&markdown_cell(name));
            output.push_str(" | ");
            output.push_str(status.as_str());
            output.push_str(" | ");
            output.push_str(evidence_note(*cu_id, status));
            output.push_str(" |\n");
        }
        output.push('\n');
    }

    output
}

fn markdown_cell(value: &str) -> String {
    value
        .replace(['\n', '\r'], " ")
        .replace(['–', '—'], "-")
        .replace('’', "'")
}

fn classify_cu(cu_id: u32) -> EvidenceStatus {
    if time_sync_gaps().contains(&cu_id) {
        return EvidenceStatus::Gap;
    }
    if implemented_cus().contains(&cu_id) {
        return EvidenceStatus::Implemented;
    }
    if partial_cus().contains(&cu_id) {
        return EvidenceStatus::Partial;
    }
    EvidenceStatus::NeedsProof
}

fn evidence_note(cu_id: u32, status: EvidenceStatus) -> &'static str {
    match status {
        EvidenceStatus::Implemented => {
            "Existing tests/docs provide direct evidence; verify CU tag coverage."
        }
        EvidenceStatus::Partial => {
            "Implementation or broad tests exist, but CU-specific proof is incomplete."
        }
        EvidenceStatus::Gap => {
            "No direct server-profile proof found; requires implementation or explicit profile exclusion."
        }
        EvidenceStatus::NeedsProof => {
            "Generated nodes or broad coverage may exist; add CU-specific evidence."
        }
        EvidenceStatus::SourceIssue if cu_id == 5592 => {
            "Referenced by closure but absent from conformance_units."
        }
        EvidenceStatus::SourceIssue => "Referenced by closure but absent from conformance_units.",
    }
}

fn implemented_cus() -> BTreeSet<u32> {
    [
        2317, 2328, 2352, 2371, 2389, 2600, 2837, 2853, 2963, 3072, 3073, 3080, 3143, 3175, 3530,
        3721, 3727, 3913, 3923, 3985, 5207, 5208, 5814,
    ]
    .into_iter()
    .collect()
}

fn partial_cus() -> BTreeSet<u32> {
    [
        2231, 2400, 2407, 2820, 2936, 3147, 3184, 3186, 3192, 3545, 3554, 3560, 3808, 3911, 3912,
        3983, 4053, 4237, 5240,
    ]
    .into_iter()
    .collect()
}

fn time_sync_gaps() -> BTreeSet<u32> {
    [2478, 2479, 2480, 2786, 3802, 5505, 5793]
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{generate_markdown_report, parse_snapshot};

    const FIXTURE: &str = r#"
{
  "canonical_profiles": {
    "nano": {
      "display_name": "Nano Embedded Device 2025 Server Profile",
      "name": "Nano Embedded Device 2025 Server Profile",
      "opc_id": 2266,
      "opc_profile_uri": "http://opcfoundation.org/UA-Profile/Server/NanoEmbeddedDevice2025"
    }
  },
  "conformance_units": [
    {"opc_id": 2478, "name": "Time Sync - OS based support"},
    {"opc_id": 3912, "name": "Base Info Server Capabilities 2"}
  ],
  "relationships": {
    "transitive_cu_closure": {
      "2266": [2478, 3912, 5592]
    }
  }
}
"#;

    #[test]
    fn report_classifies_cu_gaps_partials_and_source_issues() {
        let snapshot = parse_snapshot(FIXTURE).expect("fixture parses");
        let report = generate_markdown_report(&snapshot);

        assert!(report.contains("Nano Embedded Device 2025 Server Profile"));
        assert!(report.contains("| 2478 | Time Sync - OS based support | gap |"));
        assert!(report.contains("| 3912 | Base Info Server Capabilities 2 | partial |"));
        assert!(report.contains("| 5592 | Missing from normalized CU list | source-issue |"));
    }

    #[test]
    fn report_normalizes_canonical_names_to_ascii_markdown() {
        let fixture = FIXTURE.replace(
            "Time Sync - OS based support",
            "Time Sync – OS based support",
        );
        let snapshot = parse_snapshot(&fixture).expect("fixture parses");
        let report = generate_markdown_report(&snapshot);

        assert!(report.contains("| 2478 | Time Sync - OS based support | gap |"));
    }
}
